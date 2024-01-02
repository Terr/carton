// Copyright 2023 Arjen Verstoep
// SPDX-License-Identifier: Apache-2.0

use std::ffi::CString;
use std::path::{Path, PathBuf};

use log::{error, info, warn};

use nix::mount;
use nix::sched::{self, CloneFlags};
use nix::sys::signal::Signal::SIGCHLD;
use nix::sys::wait;
use nix::unistd;

use crate::error::CartonError;
use crate::namespace::setup_namespaces;

#[derive(Default, Debug)]
pub struct Container {
    /// The current state of the container.
    pub(crate) state: ContainerState,
    /// PID of process that essentially is the container.
    pub pid: Option<unistd::Pid>,

    pub(crate) config: ContainerConfiguration,
    pub(crate) buffer: ContainerBuffer,
}

impl Container {
    pub fn run(&mut self) -> Result<(), CartonError> {
        if let ContainerState::Running = self.state {
            return Err(CartonError::AlreadyRunning);
        }

        self.config.validate()?;

        let pid = unsafe {
            // There are some issues with nix's clone() regarding ownership of the stack memory and
            // whatever is passed into the `cb` callback function. The solution is to call libc's
            // clone() directly and do some juggling with raw C pointers. Maybe another time.
            //
            // See:
            // * https://github.com/nix-rust/nix/issues/919
            // * https://github.com/nix-rust/nix/pull/920
            sched::clone(
                Box::new(|| {
                    // TODO create cgroup, set limits

                    setup_namespaces(&self.config).expect("container namespaces setup");
                    unistd::chdir("/").unwrap();
                    execute_command(
                        self.config
                            .command
                            .as_ref()
                            .expect("command should not be None at this point"),
                        &self.config.arguments,
                    )
                }),
                &mut self.buffer.stack,
                CloneFlags::CLONE_NEWUTS | CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID,
                Some(SIGCHLD as i32),
            )?
        };
        self.pid = Some(pid);
        self.state = ContainerState::Running;

        Ok(())
    }

    pub fn wait_for_exit(&mut self) {
        match wait::waitpid(self.pid, None) {
            Ok(wait::WaitStatus::Exited(_, exit_code)) => {
                info!("Process exited with exit code {}", exit_code)
            }
            Ok(status) => warn!("Process reported this instead of exiting: {:?}", status),
            Err(e) => error!(
                "Error while waiting for child (did it already exit?) {:#?}",
                e
            ),
        };

        self.pid = None;
        self.state = ContainerState::Exited;
    }
}

#[derive(Default, Debug)]
pub(crate) struct ContainerConfiguration {
    /// The path to the root filesystem of the container.
    pub(crate) rootfs: Option<Mount>,
    /// Command to execute inside the container.
    pub(crate) command: Option<PathBuf>,
    /// Arguments to the command
    pub(crate) arguments: Vec<String>,
    /// Vita paths (like /proc, /tmp, /dev) and paths from the "host" to bind mount inside the container.
    pub(crate) mounts: Vec<Mount>,
    /// Device nodes to create in /dev.
    pub(crate) devices: Vec<DeviceNode>,
}

impl ContainerConfiguration {
    pub(crate) fn validate(&self) -> Result<(), CartonError> {
        match &self.rootfs {
            None => return Err(CartonError::MissingRequiredConfiguration("rootfs".into())),
            Some(root_spec) => {
                if !root_spec
                    .source
                    .as_ref()
                    .expect("rootfs source path should not be None")
                    .is_dir()
                {
                    return Err(CartonError::InvalidConfiguration(format!(
                        "rootfs does not exist or is not a directory: {:?}",
                        root_spec.source
                    )));
                }
            }
        };

        if self.command.is_none() {
            return Err(CartonError::MissingRequiredConfiguration("command".into()));
        }

        Ok(())
    }
}

#[derive(Default, Debug)]
pub struct ContainerBuffer {
    /// The buffer that contains the container's process stack. If this is too small
    /// then stack overflows will happen.
    pub(crate) stack: Vec<u8>,
}

#[derive(Default, Debug)]
pub enum ContainerState {
    #[default]
    NotCreated,
    Running,
    Exited,
}

#[derive(Debug)]
pub struct Mount {
    pub(crate) source: Option<PathBuf>,
    relative_target: PathBuf,
    fstype: Option<String>,
    flags: mount::MsFlags,
    data: Option<String>,
}

impl Mount {
    /// Defines a "bind" mount which can be used to share a directory from outside the container
    /// with the container.
    pub(crate) fn bind(
        source: PathBuf,
        relative_target: PathBuf,
        flags: Option<mount::MsFlags>,
        data: Option<String>,
    ) -> Self {
        Mount {
            source: Some(source),
            relative_target,
            fstype: None,
            flags: flags.unwrap_or(mount::MsFlags::MS_BIND | mount::MsFlags::MS_PRIVATE),
            data,
        }
    }

    /// When the container runs in a separate PID namespace it also needs a separate /proc mount that
    /// will contain only this PID namespace's processes.
    pub(crate) fn procfs(relative_target: PathBuf) -> Self {
        Mount {
            source: None::<PathBuf>,
            relative_target,
            fstype: Some("proc".into()),
            flags: mount::MsFlags::empty(),
            data: None,
        }
    }

    pub(crate) fn rootfs(source: PathBuf) -> Self {
        Mount {
            source: Some(source),
            relative_target: "".into(),
            fstype: None,
            flags: mount::MsFlags::MS_BIND | mount::MsFlags::MS_PRIVATE,
            data: None,
        }
    }

    pub(crate) fn tmpfs(relative_target: PathBuf) -> Self {
        Mount {
            source: None::<PathBuf>,
            relative_target,
            fstype: Some("tmpfs".into()),
            flags: mount::MsFlags::empty(),
            data: None,
        }
    }

    /// Returns the absolute path where the mount has been mounted
    pub(crate) fn mount(&self, rootfs_path: &Path) -> Result<PathBuf, CartonError> {
        let mount_path = rootfs_path.join(&self.relative_target);

        info!(
            "mount {} ({}) at {}",
            &self
                .source
                .as_ref()
                .map_or("(no source)", |p| p.to_str().unwrap()),
            self.fstype.as_ref().map_or("bind mount", |f| f.as_str()),
            &mount_path.display()
        );

        mount::mount(
            self.source.as_ref(),
            &mount_path,
            self.fstype.as_deref(),
            self.flags,
            self.data.as_deref(),
        )?;

        Ok(mount_path)
    }
}

#[derive(Debug)]
pub(crate) struct DeviceNode {
    /// Path to the device node under "/dev/" (don't include this prefix)
    pub path: PathBuf,
    /// Major device type number (e.g. 1 for /dev/null)
    pub major: u64,
    /// Minor device type number (e.g. 5 for /dev/null)
    pub minor: u64,
}

fn execute_command(command: &Path, arguments: &[String]) -> isize {
    let Ok(c_cmd) = CString::new(command.to_str().unwrap()) else {
        return 126;
    };
    let mut c_args = arguments
        .iter()
        .map(|arg| CString::new(arg.as_str()).expect("valid C string-like argument"))
        .collect::<Vec<CString>>();
    c_args.insert(0, c_cmd.clone());

    // This syscall replaces the current process with the requested command. That means that this
    // `run_command()` function will only return if something went wrong with starting the command.
    // TODO execve()
    unistd::execv(&c_cmd, &c_args).and(Ok(0)).unwrap()
}

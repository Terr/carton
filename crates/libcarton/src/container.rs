// Copyright 2023 Arjen Verstoep
// SPDX-License-Identifier: Apache-2.0

use std::ffi::CString;
use std::path::{Path, PathBuf};

use log::{error, info, warn};

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
    pub(crate) rootfs: Option<MountSpecification>,
    /// Command to execute inside the container.
    pub(crate) command: Option<PathBuf>,
    /// Arguments to the command
    pub(crate) arguments: Vec<String>,
    /// Additional paths on the "host" to bind mount inside the container.
    pub(crate) mounts: Vec<MountSpecification>,
    /// Device nodes to create in /dev.
    pub(crate) devices: Vec<DeviceNode>,
}

impl ContainerConfiguration {
    pub(crate) fn validate(&self) -> Result<(), CartonError> {
        match &self.rootfs {
            None => return Err(CartonError::MissingRequiredConfiguration("rootfs".into())),
            Some(root_spec) => {
                if !root_spec.source.is_dir() {
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

#[derive(Default, Debug)]
pub(crate) struct MountSpecification {
    pub source: PathBuf,
    pub destination: PathBuf,
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
    // `run_command()` function will only return if went wrong with starting the command.
    // TODO execve()
    unistd::execv(&c_cmd, &c_args).and(Ok(0)).unwrap()
}

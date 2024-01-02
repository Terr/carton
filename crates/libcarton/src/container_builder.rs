// Copyright 2023 Arjen Verstoep
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use nix::sys::resource;

use crate::consts::DEFAULT_CONTAINER_STACK_SIZE;
use crate::container::{Container, ContainerBuffer, ContainerConfiguration, DeviceNode, Mount};
use crate::error::CartonError;

#[derive(Default, Debug)]
pub struct ContainerBuilder {
    stack_size: Option<u64>,
    config: ContainerConfiguration,
}

impl ContainerBuilder {
    pub fn new() -> Self {
        ContainerBuilder::default()
    }

    pub fn rootfs(mut self, path: PathBuf) -> Self {
        self.config.rootfs = Some(Mount::rootfs(path));

        self
    }

    pub fn command(mut self, command: PathBuf, args: Option<Vec<String>>) -> Self {
        self.config.command = Some(command);
        self.config.arguments = args.unwrap_or_default();
        self
    }

    pub fn stack_size(mut self, size: u64) -> Self {
        self.stack_size = Some(size);
        self
    }

    /// Adds mounting configuration for some important mounts, in the correct order.
    pub fn add_default_mounts(mut self) -> Self {
        self.config.mounts.extend(vec![
            Mount::procfs("proc".into()),
            Mount::tmpfs("tmp".into()),
            Mount::tmpfs("dev".into()),
        ]);

        self
    }

    pub fn add_mount(mut self, source: PathBuf, relative_target: PathBuf) -> Self {
        self.config
            .mounts
            .push(Mount::bind(source, relative_target, None, None));
        self
    }

    pub fn add_default_devices(mut self) -> Self {
        self.config.devices.extend([
            DeviceNode {
                path: "null".into(),
                major: 1,
                minor: 3,
            },
            DeviceNode {
                path: "zero".into(),
                major: 1,
                minor: 5,
            },
            DeviceNode {
                path: "full".into(),
                major: 1,
                minor: 7,
            },
            DeviceNode {
                path: "tty".into(),
                major: 5,
                minor: 0,
            },
            DeviceNode {
                path: "urandom".into(),
                major: 1,
                minor: 9,
            },
            DeviceNode {
                path: "random".into(),
                major: 1,
                minor: 8,
            },
        ]);

        self
    }

    pub fn add_device(mut self, path: &Path, major: u64, minor: u64) -> Self {
        self.config.devices.push(DeviceNode {
            path: path.into(),
            major,
            minor,
        });

        self
    }

    pub fn build(self) -> Result<Container, CartonError> {
        let stack_size = self.determine_stack_size();

        Ok(Container {
            config: self.config,
            buffer: ContainerBuffer {
                stack: vec![0; stack_size],
            },
            ..Default::default()
        })
    }

    fn determine_stack_size(&self) -> usize {
        self.stack_size
            .or_else(|| {
                resource::getrlimit(resource::Resource::RLIMIT_STACK)
                    .map(|(soft_limit, _)| soft_limit)
                    .ok()
            })
            .map(|size| {
                if size == u64::MAX {
                    // In this case getrlimit() gave back an 'unlimited' limit or it was explicitly
                    // set like this.
                    // Since we can't create a buffer this big just create a standard sized one.
                    DEFAULT_CONTAINER_STACK_SIZE
                } else {
                    size as usize
                }
            })
            .unwrap_or(DEFAULT_CONTAINER_STACK_SIZE)
    }
}

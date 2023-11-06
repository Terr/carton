// Copyright 2023 Arjen Verstoep
// SPDX-License-Identifier: Apache-2.0

use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CartonError {
    #[error("missing a required configuration value: `{0}`")]
    MissingRequiredConfiguration(String),
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(String),
    #[error("container already running")]
    AlreadyRunning,
    #[error("syscall failed: {0}")]
    SysCallFailed(String),
    #[error("namespace error: {0}")]
    NamespaceError(String),
}

impl From<nix::Error> for CartonError {
    fn from(error: nix::Error) -> Self {
        CartonError::SysCallFailed(format!("{} ({})", error.desc(), error))
    }
}

// Copyright 2023 Arjen Verstoep
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use anyhow::{Context, Result};

use clap::Parser;

use log::info;

use libcarton::ContainerBuilder;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// The root filesystem of the container
    rootfs_path: PathBuf,
    /// The command in the root filesystem to run inside the container
    command: PathBuf,
    /// Arguments to the command
    arguments: Option<Vec<String>>,
}

fn main() -> Result<()> {
    env_logger::init();

    let cli_args = Args::parse();

    let mut container = ContainerBuilder::new()
        .rootfs(cli_args.rootfs_path)
        .command(cli_args.command, cli_args.arguments)
        .add_default_mounts()
        .add_default_devices()
        .build()
        .context("building container")?;

    info!("Starting container");
    container.run()?;

    info!("Waiting for container to exit");
    container.wait_for_exit();

    Ok(())
}

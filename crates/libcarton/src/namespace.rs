// Copyright 2023 Arjen Verstoep
// SPDX-License-Identifier: Apache-2.0

use log::info;

use nix::mount;
use nix::sys::stat;
use nix::unistd;

use crate::container::{ContainerConfiguration, DeviceNode, MountSpecification};
use crate::error::CartonError;

/// Does the entire dance of setting up all the elements of the new processes' namespace, like
/// creating devices nodes and actually mounting the root partition.
pub(crate) fn setup_namespaces(config: &ContainerConfiguration) -> Result<(), CartonError> {
    prepare_rootfs(&config.rootfs)?;

    mount_procfs(&config.rootfs)?;
    mount_tmp(&config.rootfs)?;
    mount_additional_binds(&config.rootfs, &config.mounts)?;

    mount_dev(&config.rootfs)?;
    create_device_nodes(&config.rootfs, &config.devices)?;

    mount_rootfs(&config.rootfs)?;

    Ok(())
}

/// Before we can set up mounts for the new root filesystem we need to prepare both the root mount
/// point and the source directory containing the new root filesystem.
///
/// If we don't do this first, further mounts will either not pass into the mount namespace after
/// pivot_root() or affect the "host" system, messing up things.
fn prepare_rootfs(root_spec: &MountSpecification) -> Result<(), CartonError> {
    // Remount root within our mount namespace and mark it as private, so that any changes to it
    // (like a umount) will not (try) to affect the real root partition.
    mount::mount(
        None::<&str>,
        "/",
        None::<&str>,
        mount::MsFlags::MS_REC | mount::MsFlags::MS_PRIVATE,
        None::<&str>,
    )?;

    // Prepare the new root filesystem for mounting
    mount::mount(
        Some(&root_spec.source),
        &root_spec.source,
        None::<&str>,
        mount::MsFlags::MS_BIND | mount::MsFlags::MS_PRIVATE,
        None::<&str>,
    )?;

    Ok(())
}

/// When the container runs in a separate PID namespace it also needs a separate /proc mount that
/// will contain only this PID namespace's processes.
fn mount_procfs(root_mount: &MountSpecification) -> Result<(), CartonError> {
    let proc_mount = &root_mount.source.join("proc");
    info!("mounting proc at {:?}", proc_mount);

    mount::mount(
        None::<&str>,
        proc_mount,
        Some("proc"),
        mount::MsFlags::empty(),
        None::<&str>,
    )?;

    Ok(())
}

/// Mounts /tmp under the new rootfs
fn mount_tmp(root_mount: &MountSpecification) -> Result<(), CartonError> {
    let tmp_mount = &root_mount.source.join("tmp");
    info!("mounting tmp at {:?}", tmp_mount);

    mount::mount(
        None::<&str>,
        tmp_mount,
        Some("tmpfs"),
        mount::MsFlags::empty(),
        None::<&str>,
    )?;

    Ok(())
}

fn mount_additional_binds(
    root_mount: &MountSpecification,
    mounts: &[MountSpecification],
) -> Result<(), CartonError> {
    for mount_spec in mounts {
        mount::mount(
            Some(&mount_spec.source),
            &root_mount.source.join(&mount_spec.destination),
            None::<&str>,
            mount::MsFlags::MS_BIND | mount::MsFlags::MS_PRIVATE,
            None::<&str>,
        )?;
    }

    Ok(())
}

/// Mounts a clean /dev under the new rootfs
fn mount_dev(root_spec: &MountSpecification) -> Result<(), CartonError> {
    let dev_path = root_spec.source.join("dev");
    info!("mount /dev at {:?}", &dev_path);

    mount::mount(
        None::<&str>,
        &dev_path,
        Some("tmpfs"),
        mount::MsFlags::empty(),
        None::<&str>,
    )?;

    Ok(())
}

fn create_device_nodes(
    root_spec: &MountSpecification,
    devices: &[DeviceNode],
) -> Result<(), CartonError> {
    let dev_path = root_spec.source.join("dev");

    let device_perm = stat::Mode::from_bits(0o0666).unwrap();
    for node in devices {
        stat::mknod(
            &dev_path.join(&node.path),
            stat::SFlag::S_IFCHR,
            device_perm,
            stat::makedev(node.major, node.minor),
        )?;
    }

    // These are symlinks from /proc on the "old" (current) root filesystem
    unistd::symlinkat("/proc/self/fd", None, &dev_path.join("fd"))?;
    unistd::symlinkat("/proc/self/fd/0", None, &dev_path.join("stdin"))?;
    unistd::symlinkat("/proc/self/fd/1", None, &dev_path.join("stdout"))?;
    unistd::symlinkat("/proc/self/fd/2", None, &dev_path.join("stderr"))?;

    Ok(())
}

/// Replacing the root mount inside the contaier consists of a few steps. This function marks all
/// mount points with the right flags and then does the all-important `pivot_root()` that replaces
/// the root mount inside the container with the new root filesystem.
fn mount_rootfs(root_spec: &MountSpecification) -> Result<(), CartonError> {
    // Pivot root to the new bind mount
    //
    // Instead of using a temporary "put_old" directory to mount the current root on, like
    // suggested by pivot_root(8) manpage, just remount it again at /.
    //
    // This stacks the mounts with the "old root" at the top of the stack. By umounting that layer
    // we get to the new "fake" root like we want, without having to create/delete a temporary
    // directory.
    unistd::pivot_root(&root_spec.source, &root_spec.source)?;

    // Re-mount the root yet again but mark it as "MS_SLAVE" so umount events will
    // in no circumstance propagate to outside the namespace.
    // See: https://github.com/opencontainers/runc/pull/1500.
    mount::mount(
        None::<&str>,
        "/",
        None::<&str>,
        mount::MsFlags::MS_SLAVE | mount::MsFlags::MS_REC,
        None::<&str>,
    )?;

    // Unmount the "old" root filesystem which is currently sitting on top of the mount stack
    // (the "/" endpoint has been mounted multiple times at this point)
    mount::umount2("/", mount::MntFlags::MNT_DETACH)?;

    Ok(())
}

# Carton

A very simple, lightweight (hence the name) container runtime.

This is a learning project for myself to learn about the inner workings of Linux containers. I have been using them for a long time but how do they actually work?

It turns out that it only takes a few Linux syscalls to turn a process into a very minimal "container", which actually just means a process is placed into separate [namespaces][3]. Most of the system calls have to do with setting up mounts so that your process will have its own root filesystem.

If you found this repository because you're curious about containers too then I hope this code and the comments will help you.

## How to run a container

1. You need a "root filesystem" for your container. Because this runtime doesn't work with Docker images you need to have a directory somewhere with, for example, the contents of an [Alpine mini root filesystem][4]
2. [Download a release][5] or compile this project using Cargo
3. As a root user or with sudo, run something like `carton /path/to/alpine_minirootfs /bin/sh`
4. Enjoy your namespaced process!

## Features I'd like to add

Even though this will never be a full-fledged [OCI compliant][2] container runtime, I would still like to add some features to see how they work:

* A cgroup for the container process, and options to limit the memory and CPU usage.
* Network namespace
* Reduced [capabilities][1] when running a container as root
* Running unprivileged containers
* Ability to start multiple detached containers and interact with them via a daemon process (ala dockerd)

## Development

For any container runtime development I highly recommend to do testing and debugging inside a virtual machine, because any mistake with mounting `/`, `/tmp`, etc. will cause your host system(d) to malfunction, probably forcing you to reboot (don't ask me how I know.)

[1]: https://man7.org/linux/man-pages/man7/capabilities.7.html
[2]: https://github.com/opencontainers/runtime-spec/blob/main/spec.md
[3]: https://man7.org/linux/man-pages/man7/namespaces.7.html
[4]: https://alpinelinux.org/downloads/
[5]: https://github.com/Terr/carton/releases

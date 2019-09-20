pH Hypervisor
=============

pH is a KVM hypervisor for Linux written in the Rust programming language as a tool
for isolating desktop application running environments in Subgraph OS.

Building pH
-----------

    $ sudo apt install wget bc libelf-dev pkg-config libwayland-dev libxcb-composite0-dev libxkbcommon-dev libgbm-dev libpixman-1-dev libdrm-dev libdbus-1-dev
    $ cargo build --release

Running pH
----------

After successfully building pH the only artifact you need is the pH binary itself. You can copy it
anywhere and use it.

If you run pH in citadel, you can simply pass it the name of a realm:

    $ ./pH --realm main

This will use the correct realmfs image as a block device for the root filesystem and
mount the realm home directory as a 9p filesystem.

Without any arguments, pH will self-host on the current filesystem by mounting the
root directory as a read-only 9p filesystem. Currently it is assumed that the
home directory is /home/user and if you have a different home directory you'll
need to tell pH about it:

    $ ./pH --home /home/citadel

By default a shell inside the pH instance will be launched as the user account, 
but you can also add --root flag to launch a root shell instead:

    $ ./pH --home /home/citadel --root

Devices
-------

The hardware emulation in pH consists of a minimal set of small legacy devices needed to 
boot the system as well as several Virtio devices.  Virtio is a standard interface
for efficient communication between a guest operating system and a hypervisor such that
the same guest drivers will work with any hypervisor implemention of the emulated
virtio devices.


### virtio-block

A block device driver.

#### Disk Images

Raw ext4 disk images are supported, as well as realmfs images, but currently they
are not mounted with dm-verity.

### virtio-9p

A 9P filesystem server which can be used to mount filesystem trees on the host into
the guest.

### virtio-rng

Provides entropy from /dev/urandom on the host to the guest.

### virtio-serial

A serial port device which is used to provide an interactive console on the guest.

### virtio-wl

Proxies Wayland messages from the guest to a wayland compositor running on the host. Also
allocates and shares memory and DMA-Buf allocations into the guest.



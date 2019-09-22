use std::{result, io, fmt};

pub enum Error {
    Pid1,
    KernelCmdLine(io::Error),
    NoRootVar,
    NoRootFsVar,
    RootFsMount(String, io::Error),
    MountProcFS(io::Error),
    MountTmpFS(String, io::Error),
    MountSysFS(io::Error),
    MountCGroup(io::Error),
    MountDevTmpFS(io::Error),
    MountDevPts(io::Error),
    MountOverlay(io::Error),
    MoveMount(String, String, io::Error),
    Mount9P(String, String, io::Error),
    Umount(String, io::Error),
    MkDir(String, io::Error),
    SetHostname(io::Error),
    SetSid(io::Error),
    SetControllingTty(io::Error),
    PivotRoot(String, String, io::Error),
    WaitPid(io::Error),
    WriteEtcHosts(io::Error),
    RunShell(io::Error),
    CStringConv,
    ChmodFailed(io::Error),
    ChownFailed(io::Error),
    LaunchFailed(String, io::Error),
    RebootFailed(io::Error),
    OpenLogFailed(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Error::*;
        match self {
            Pid1 => write!(f, "not running as pid 1"),
            KernelCmdLine(err) => write!(f, "failed to load kernel command line from /proc/cmdline: {}", err),
            NoRootVar => write!(f, "Cannot mount rootfs because no phinit.root is set"),
            NoRootFsVar => write!(f, "Cannot mount rootfs because no phinit.rootfs is set"),
            RootFsMount(rootfs, err) =>  write!(f, "Failed to mount rootfs {}: {}", rootfs, err),
            MountProcFS(err) => write!(f, "unable to mount procfs: {}", err),
            MountTmpFS(target,err) => write!(f, "failed to mount tmpfs at {}: {}", target, err),
            MountSysFS(err) => write!(f, "failed to mount sysfs at /sys: {}", err),
            MountCGroup(err) => write!(f, "failed to mount cgroup at /sys/fs/cgroup: {}", err),
            MountDevTmpFS(err) => write!(f, "failed to mount devtmpfs at /dev: {}", err),
            MountDevPts(err) => write!(f, "failed to mount /dev/pts: {}", err),
            MountOverlay(err) => write!(f, "failed to mount overlayfs: {}", err),
            MoveMount(from, to, err) => write!(f, "failed to move mount from {} to {}: {}", from, to, err),
            Mount9P(tag,target, err) => write!(f, "failed to mount 9p volume {} at {}: {}", tag, target, err),
            Umount(target, err) => write!(f, "failed to unmount {}: {}", target, err),
            MkDir(target, err) => write!(f, "failed to mkdir {}: {}", target, err),
            SetHostname(err) => write!(f, "sethostname() failed: {}", err),
            SetSid(err) => write!(f, "call to setsid() failed: {}", err),
            SetControllingTty(err) => write!(f, "failed to set controlling terminal: {}", err),
            PivotRoot(newroot, putroot, err) => write!(f, "failed to pivot_root({}, {}): {}", newroot, putroot, err),
            WaitPid(err) => write!(f, "failed to waitpid(): {}", err),
            WriteEtcHosts(err) => write!(f, "failed to write /etc/hosts: {}", err),
            RunShell(err) => write!(f, "error launching shell: {}", err),
            CStringConv => write!(f, "failed to create CString"),
            ChmodFailed(err) => write!(f, "failed to chmod: {}", err),
            ChownFailed(err) => write!(f, "failed to chown: {}", err),
            LaunchFailed(exec, err) => write!(f, "unable to execute {}: {}", exec, err),
            RebootFailed(err) => write!(f, "could not reboot system: {}", err),
            OpenLogFailed(err) => write!(f, "failed to open log file: {}", err),
        }
    }
}

pub type Result<T> = result::Result<T, Error>;
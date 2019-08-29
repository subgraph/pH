use libc::{self, c_ulong, c_void};
use std::os::unix::io::RawFd;
use crate::vm::{Error,Result};

pub const IOC_SIZEBITS:  u64 = 14;
pub const IOC_DIRBITS:   u64 = 2;

pub const IOC_NONE:      u64 = 0;
pub const IOC_READ:      u64 = 2;
pub const IOC_WRITE:     u64 = 1;
pub const IOC_RDWR:      u64 = IOC_READ | IOC_WRITE;
pub const IOC_NRBITS:    u64 = 8;
pub const IOC_TYPEBITS:  u64 = 8;
pub const IOC_NRSHIFT:   u64 = 0;
pub const IOC_TYPESHIFT: u64 = IOC_NRSHIFT + IOC_NRBITS;
pub const IOC_SIZESHIFT: u64 = IOC_TYPESHIFT + IOC_TYPEBITS;
pub const IOC_DIRSHIFT:  u64 = IOC_SIZESHIFT + IOC_SIZEBITS;

pub const IOC_NRMASK:    u64 = (1 << IOC_NRBITS) - 1;
pub const IOC_TYPEMASK:  u64 = (1 << IOC_TYPEBITS) - 1;
pub const IOC_SIZEMASK:  u64 = (1 << IOC_SIZEBITS) - 1;
pub const IOC_DIRMASK:   u64 = (1 << IOC_DIRBITS) - 1;

macro_rules! ioc {
    ($dir:expr, $ty:expr, $nr:expr, $sz:expr) => (
       ((($dir as u64 & $crate::system::ioctl::IOC_DIRMASK) << $crate::system::ioctl::IOC_DIRSHIFT) |
        (($ty as u64 & $crate::system::ioctl::IOC_TYPEMASK) << $crate::system::ioctl::IOC_TYPESHIFT) |
        (($nr as u64 & $crate::system::ioctl::IOC_NRMASK) << $crate::system::ioctl::IOC_NRSHIFT) |
        (($sz as u64 & $crate::system::ioctl::IOC_SIZEMASK) << $crate::system::ioctl::IOC_SIZESHIFT)) as c_ulong)
}

macro_rules! io {
    ($ty:expr, $nr:expr) => (ioc!($crate::system::ioctl::IOC_NONE, $ty, $nr, 0))
}

macro_rules! iow {
    ($ty:expr, $nr:expr, $sz:expr) => (ioc!($crate::system::ioctl::IOC_WRITE, $ty, $nr, $sz))
}

macro_rules! ior {
    ($ty:expr, $nr:expr, $sz:expr) => (ioc!($crate::system::ioctl::IOC_READ, $ty, $nr, $sz))
}

macro_rules! iorw {
    ($ty:expr, $nr:expr, $sz:expr) => (ioc!($crate::system::ioctl::IOC_RDWR, $ty, $nr, $sz))
}

pub unsafe fn ioctl_with_val(fd: RawFd, request: c_ulong, val: c_ulong) -> Result<u32> {
    let ret = libc::ioctl(fd, request, val);
    if ret < 0 {
        return Err(Error::from_last_errno());
    }
    Ok(ret as u32)
}

pub unsafe fn ioctl_with_ref<T>(fd: RawFd, request: c_ulong, arg: &T) -> Result<u32> {
    let ret = libc::ioctl(fd, request, arg as *const T as *const c_void);
    if ret < 0 {
        return Err(Error::from_last_errno());
    }
    Ok(ret as u32)
}

pub unsafe fn ioctl_with_mut_ref<T>(fd: RawFd, request: c_ulong, arg: &mut T) -> Result<u32> {
    let ret = libc::ioctl(fd, request, arg as *mut T as *mut c_void);
    if ret < 0 {
        return Err(Error::from_last_errno());
    }
    Ok(ret as u32)
}


use std::os::unix::io::RawFd;
use libc::{self, c_ulong};

use crate::system::ioctl::{ioctl_with_ref, ioctl_with_mut_ref, ioctl_with_val};
use crate::vm::arch::{Error,Result};

const KVMIO:     u64 = 0xAE;

pub const KVM_GET_SUPPORTED_CPUID: libc::c_ulong       = iorw!   (KVMIO, 0x05, 8);
pub const KVM_SET_CPUID2: libc::c_ulong                = iow!    (KVMIO, 0x90, 8);
pub const KVM_SET_TSS_ADDR: c_ulong              = io!     (KVMIO, 0x47);
pub const KVM_CREATE_PIT2: c_ulong               = iow!    (KVMIO, 0x77, 64);
pub const KVM_SET_FPU: c_ulong                   = iow!    (KVMIO, 0x8d, 416);
pub const KVM_SET_MSRS: c_ulong                  = iow!    (KVMIO, 0x89, 8);
pub const KVM_GET_SREGS: c_ulong                 = ior!    (KVMIO, 0x83, 312);
pub const KVM_SET_SREGS: c_ulong                 = iow!    (KVMIO, 0x84, 312);
pub const KVM_GET_LAPIC: c_ulong                 = ior!    (KVMIO, 0x8e, 1024);
pub const KVM_SET_LAPIC: c_ulong                 = iow!    (KVMIO, 0x8f, 1024);

pub fn call_ioctl_with_ref<T>(name: &'static str, fd: RawFd, request: c_ulong, arg: &T) -> Result<()> {
    unsafe {
        ioctl_with_ref(fd, request, arg)
            .map_err(|e| Error::IoctlError(name, e))?;
        Ok(())
    }
}

pub fn call_ioctl_with_mut_ref<T>(name: &'static str, fd: RawFd, request: c_ulong, arg: &mut T) -> Result<()> {
    unsafe {
        ioctl_with_mut_ref(fd, request, arg)
            .map_err(|e| Error::IoctlError(name, e))?;
        Ok(())
    }
}

pub fn call_ioctl_with_val(name: &'static str, fd: RawFd, request: c_ulong, val: c_ulong) -> Result<()> {
    unsafe {
        ioctl_with_val(fd, request, val)
            .map_err(|e| Error::IoctlError(name, e))?;
        Ok(())
    }
}



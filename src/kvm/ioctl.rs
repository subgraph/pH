use libc::{self, c_char, c_ulong};
use std::os::unix::io::RawFd;
use std::ffi::CString;

use crate::system::ioctl::{ioctl_with_val,ioctl_with_ref,ioctl_with_mut_ref};

use crate::kvm::{Result, Error};
use crate::system::ErrnoError;
use crate::vm::arch::KvmRegs;
use std::result;

const KVMIO:     u64 = 0xAE;

const KVM_GET_API_VERSION: c_ulong           = io!     (KVMIO, 0x00);
const KVM_CREATE_VM: c_ulong                 = io!     (KVMIO, 0x01);
const KVM_CHECK_EXTENSION: c_ulong           = io!     (KVMIO, 0x03);
const KVM_CREATE_IRQCHIP: c_ulong            = io!     (KVMIO, 0x60);
const KVM_GET_VCPU_MMAP_SIZE: c_ulong        = io!     (KVMIO, 0x04);
const KVM_CREATE_VCPU: c_ulong               = io!     (KVMIO, 0x41);
const KVM_SET_USER_MEMORY_REGION: c_ulong    = iow!    (KVMIO, 0x46, 32);
const KVM_IRQ_LINE: c_ulong                  = iow!    (KVMIO, 0x61, 8);
const KVM_IRQFD: c_ulong                     = iow!    (KVMIO, 0x76, 32);
const KVM_IOEVENTFD: c_ulong                 = iow!    (KVMIO, 0x79, 64);
const KVM_RUN: c_ulong                       = io!     (KVMIO, 0x80);
const KVM_GET_REGS: c_ulong                  = ior!    (KVMIO, 0x81, 144);
const KVM_SET_REGS: c_ulong                  = iow!    (KVMIO, 0x82, 144);

struct InnerFd(RawFd);
impl InnerFd {
    fn raw(&self) -> RawFd { self.0 }
}

impl Drop for InnerFd {
    fn drop(&mut self) {
        let _ = unsafe { libc::close(self.0) };
    }
}

pub struct SysFd(InnerFd);

fn raw_open_kvm() -> Result<RawFd> {
    let path = CString::new("/dev/kvm").unwrap();
    let fd = unsafe { libc::open(path.as_ptr() as *const c_char, libc::O_RDWR) };
    if fd < 0 {
        return Err(Error::OpenKvm(ErrnoError::last_os_error()));
    }
    Ok(fd)
}

impl SysFd {
    pub fn open() -> Result<SysFd> {
        let fd = raw_open_kvm()?;
        Ok(SysFd(InnerFd(fd)))
    }

    pub fn raw(&self) -> RawFd { self.0.raw() }
}

pub struct VmFd(InnerFd);

impl VmFd {
    fn new(fd: RawFd) -> VmFd {
        VmFd( InnerFd(fd) )
    }
    pub fn raw(&self) -> RawFd { self.0.raw() }
}

pub struct VcpuFd(InnerFd);

impl VcpuFd {
    fn new(fd: RawFd) -> VcpuFd {
        VcpuFd( InnerFd(fd) )
    }
    pub fn raw(&self) -> RawFd { self.0.raw() }
}


pub fn kvm_check_extension(sysfd: &SysFd, extension: u32) -> Result<u32> {
    unsafe {
        ioctl_with_val(sysfd.raw(), KVM_CHECK_EXTENSION, extension as c_ulong)
            .map_err(|e| Error::IoctlError("KVM_CHECK_EXTENSION", e))
    }
}

pub fn kvm_get_api_version(sysfd: &SysFd) -> Result<u32> {
    unsafe {
        ioctl_with_val(sysfd.raw(), KVM_GET_API_VERSION, 0)
            .map_err(|e| Error::IoctlError("KVM_GET_API_VERSION", e))
    }
}

pub fn kvm_create_vm(sysfd: &SysFd) -> Result<VmFd> {
    let fd = unsafe {
        ioctl_with_val(sysfd.raw(), KVM_CREATE_VM, 0)
            .map_err(|e| Error::IoctlError("KVM_CREATE_VM", e))?
    };
    Ok(VmFd::new(fd as RawFd))
}

pub fn kvm_get_vcpu_mmap_size(sysfd: &SysFd) -> Result<u32> {
    unsafe {
        ioctl_with_val(sysfd.raw(), KVM_GET_VCPU_MMAP_SIZE, 0)
            .map_err(|e| Error::IoctlError("KVM_GET_VCPU_MMAP_SIZE", e))
    }
}

#[repr(C)]
pub struct KvmUserspaceMemoryRegion {
    slot: u32,
    flags: u32,
    guest_phys_addr: u64,
    memory_size: u64,
    userspace_addr: u64,
}

impl KvmUserspaceMemoryRegion {
    pub fn new(slot: u32, guest_address: u64, host_address: u64, size: u64) -> KvmUserspaceMemoryRegion {
        KvmUserspaceMemoryRegion {
            slot,
            flags: 0,
            guest_phys_addr: guest_address,
            memory_size: size,
            userspace_addr: host_address,
        }
    }
}

pub fn kvm_set_user_memory_region(vmfd: &VmFd, region: &KvmUserspaceMemoryRegion) -> Result<()> {
    call_ioctl_with_ref("KVM_SET_USER_MEMORY_REGION",vmfd.raw(), KVM_SET_USER_MEMORY_REGION, region)
}

pub fn kvm_create_irqchip(vmfd: &VmFd) -> Result<()> {
    call_ioctl_with_val("KVM_CREATE_IRQCHIP", vmfd.raw(), KVM_CREATE_IRQCHIP, 0)
}

pub fn kvm_create_vcpu(vmfd: &VmFd, cpu_id: u32) -> Result<VcpuFd> {
    let fd = unsafe {
        ioctl_with_val(vmfd.raw(), KVM_CREATE_VCPU, cpu_id as c_ulong)
            .map_err(|e| Error::IoctlError("KVM_CREATE_VCPU", e))?
    };
    Ok(VcpuFd::new(fd as RawFd))
}

#[repr(C)]
pub struct KvmIrqLevel {
    irq: u32,
    level: u32,
}

impl KvmIrqLevel {
    pub fn new(irq: u32, level: u32) -> KvmIrqLevel {
        KvmIrqLevel { irq, level }
    }
}

pub fn kvm_irq_line(vmfd: &VmFd, level: &KvmIrqLevel) -> Result<()> {
    call_ioctl_with_ref("KVM_IRQ_LINE", vmfd.raw(), KVM_IRQ_LINE, level)
}

#[repr(C)]
pub struct KvmIrqFd {
    fd: u32,
    gsi: u32,
    flags: u32,
    resample_fd: u32,
    pad1: u64,
    pad2: u64,
}

impl KvmIrqFd {
    pub fn new(fd: u32, gsi: u32) -> KvmIrqFd {
        KvmIrqFd{fd, gsi, flags:0, resample_fd: 0, pad1: 0, pad2: 0}
    }
}

pub fn kvm_irqfd(vmfd: &VmFd, irqfd: &KvmIrqFd) -> Result<()> {
    call_ioctl_with_ref("KVM_IRQFD", vmfd.raw(), KVM_IRQFD, irqfd)
}

pub const IOEVENTFD_FLAG_DATAMATCH: u32 = 1;
pub const _IOEVENTFD_FLAG_PIO : u32 = 2;
pub const IOEVENTFD_FLAG_DEASSIGN: u32 = 4;

#[repr(C)]
pub struct KvmIoEventFd {
    datamatch: u64,
    addr: u64,
    len: u32,
    fd: u32,
    flags: u32,
    padding: [u8; 36],
}

impl KvmIoEventFd {
    pub fn new_with_addr_fd(addr: u64, fd: RawFd) -> KvmIoEventFd {
        KvmIoEventFd::new(0, addr, 0, fd as u32, 0)
    }

    fn new(datamatch: u64, addr: u64, len: u32, fd: u32, flags: u32) -> KvmIoEventFd {
        KvmIoEventFd{datamatch, addr, len, fd, flags, padding: [0;36]}
    }

    #[allow(dead_code)]
    pub fn set_datamatch(&mut self, datamatch: u64, len: u32) {
        self.flags |= IOEVENTFD_FLAG_DATAMATCH;
        self.datamatch = datamatch;
        self.len = len;
    }

    pub fn set_deassign(&mut self) {
        self.flags |= IOEVENTFD_FLAG_DEASSIGN;
    }
}

pub fn kvm_ioeventfd(vmfd: &VmFd, ioeventfd: &KvmIoEventFd) -> Result<()> {
    call_ioctl_with_ref("KVM_IOEVENTFD", vmfd.raw(), KVM_IOEVENTFD, ioeventfd)
}

pub fn kvm_get_regs(cpufd: &VcpuFd, regs: &mut KvmRegs) -> Result<()> {
    call_ioctl_with_mut_ref("KVM_GET_REGS", cpufd.raw(), KVM_GET_REGS, regs)
}

pub fn kvm_set_regs(cpufd: &VcpuFd, regs: &KvmRegs) -> Result<()> {
    call_ioctl_with_ref("KVM_SET_REGS", cpufd.raw(), KVM_SET_REGS, regs)
}

pub fn kvm_run(cpufd: &VcpuFd) -> Result<()> {
    call_ioctl_with_val("KVM_RUN", cpufd.raw(), KVM_RUN, 0)
}

fn call_ioctl(name: &'static str, result: result::Result<u32, ErrnoError>) -> Result<()> {
    result.map_err(|e| Error::IoctlError(name, e))?;
    Ok(())
}

fn call_ioctl_with_ref<T>(name: &'static str, fd: RawFd, request: c_ulong, arg: &T) -> Result<()> {
    unsafe {
        ioctl_with_ref(fd, request, arg)
            .map_err(|e| Error::IoctlError(name, e))?;
        Ok(())
    }
}

fn call_ioctl_with_mut_ref<T>(name: &'static str, fd: RawFd, request: c_ulong, arg: &mut T) -> Result<()> {
    unsafe {
        ioctl_with_mut_ref(fd, request, arg)
            .map_err(|e| Error::IoctlError(name, e))?;
        Ok(())
    }
}

fn call_ioctl_with_val(name: &'static str, fd: RawFd, request: c_ulong, val: c_ulong) -> Result<()> {
    unsafe {
        call_ioctl(name, ioctl_with_val(fd, request, val))
    }
}

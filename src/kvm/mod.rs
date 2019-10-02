use std::os::unix::io::RawFd;
use std::sync::Arc;

mod ioctl;
mod ioeventfd;
mod error;

pub use error::{Result,Error};
pub use ioeventfd::IoEventFd;

use crate::vm::arch::KvmRegs;

pub const KVM_CAP_IRQCHIP: u32 = 0;
pub const KVM_CAP_HLT: u32 = 1;
pub const KVM_CAP_USER_MEMORY: u32 = 3;
pub const KVM_CAP_SET_TSS_ADDR: u32 = 4;
pub const KVM_CAP_EXT_CPUID: u32 = 7;
pub const KVM_CAP_IRQ_ROUTING: u32 = 25;
pub const KVM_CAP_IRQ_INJECT_STATUS: u32 = 26;
pub const KVM_CAP_PIT2: u32 = 33;
pub const KVM_CAP_IOEVENTFD: u32 = 36;

#[derive(Clone)]
pub struct Kvm {
    sysfd: Arc<ioctl::SysFd>,
    vmfd: Arc<ioctl::VmFd>,
}

fn check_extensions(sysfd: &ioctl::SysFd, extensions: &[u32]) -> Result<()> {
    for e in extensions {
        check_extension(sysfd, *e)?;
    }
    Ok(())
}

fn check_extension(sysfd: &ioctl::SysFd, extension: u32) -> Result<()> {
    let ret = ioctl::kvm_check_extension(&sysfd, extension)?;
    if ret == 0 {
        Err(Error::MissingRequiredExtension(extension))
    } else {
        Ok(())
    }
}

fn check_version(sysfd: &ioctl::SysFd) -> Result<()> {
    let version= ioctl::kvm_get_api_version(&sysfd)?;

    if version != 12 {
        return Err(Error::BadVersion);
    }
    Ok(())
}

impl Kvm {
    pub fn open(required_extensions: &[u32]) -> Result<Kvm> {
        let sysfd = ioctl::SysFd::open()?;

        check_version(&sysfd)?;
        check_extensions(&sysfd, &required_extensions)?;

        let vmfd= ioctl::kvm_create_vm(&sysfd)?;

        Ok(Kvm{
            sysfd: Arc::new(sysfd),
            vmfd: Arc::new(vmfd),
        })
    }

    pub fn add_memory_region(&self, slot: u32, guest_address: u64, host_address: u64, size: usize) -> Result<()> {

        let region = ioctl::KvmUserspaceMemoryRegion::new(slot, guest_address, host_address, size as u64);
        ioctl::kvm_set_user_memory_region(&self.vmfd, &region)?;
        Ok(())
    }

    pub fn remove_memory_region(&self, slot: u32) -> Result<()> {
        let region = ioctl::KvmUserspaceMemoryRegion::new(slot, 0, 0, 0);
        ioctl::kvm_set_user_memory_region(&self.vmfd, &region)?;
        Ok(())
    }

    pub fn create_irqchip(&self) -> Result<()> {
        ioctl::kvm_create_irqchip(&self.vmfd)?;
        Ok(())
    }

    pub fn irq_line(&self, irq: u32, level: u32) -> Result<()> {
        let irq_level = ioctl::KvmIrqLevel::new(irq, level);
        ioctl::kvm_irq_line(&self.vmfd, &irq_level)?;
        Ok(())
    }

    pub fn irqfd(&self, fd: u32, gsi: u32) -> Result<()> {
        let irqfd = ioctl::KvmIrqFd::new(fd, gsi);
        ioctl::kvm_irqfd(&self.vmfd, &irqfd)
    }

    pub fn ioeventfd_add(&self, address: u64, fd: RawFd) -> Result<()> {
        // XXX check for zero length capability
        let ioeventfd = ioctl::KvmIoEventFd::new_with_addr_fd(address, fd);
        ioctl::kvm_ioeventfd(&self.vmfd, &ioeventfd)
    }

    pub fn ioeventfd_del(&self, address: u64, fd: RawFd) -> Result<()> {
        let mut ioeventfd = ioctl::KvmIoEventFd::new_with_addr_fd(address, fd);
        ioeventfd.set_deassign();
        ioctl::kvm_ioeventfd(&self.vmfd, &ioeventfd)
    }

    pub fn new_vcpu(&self, id: usize) -> Result<KvmVcpu> {
        let cpufd = ioctl::kvm_create_vcpu(&self.vmfd, id as u32)?;
        Ok(KvmVcpu::new(id, Arc::new(cpufd), self.sysfd.clone()))
    }

    pub fn vmfd(&self) -> RawFd {
        self.vmfd.raw()
    }
}

#[derive(Clone)]
pub struct KvmVcpu {
    id: usize,
    cpufd: Arc<ioctl::VcpuFd>,
    sysfd: Arc<ioctl::SysFd>,
}

impl KvmVcpu {
    fn new(id: usize, cpufd: Arc<ioctl::VcpuFd>, sysfd: Arc<ioctl::SysFd>) -> KvmVcpu {
        KvmVcpu { id, cpufd, sysfd }
    }

    pub fn raw_fd(&self) -> RawFd {
        self.cpufd.raw()
    }

    pub fn sys_raw_fd(&self) -> RawFd {
        self.sysfd.raw()
    }

    pub fn get_regs(&self) -> Result<KvmRegs> {
        let mut regs = KvmRegs::new();
        ioctl::kvm_get_regs(&self.cpufd, &mut regs)?;
        Ok(regs)
    }

    pub fn set_regs(&self, regs: &KvmRegs) -> Result<()> {
        ioctl::kvm_set_regs(&self.cpufd, regs)?;
        Ok(())
    }

    pub fn run(&self) -> Result<()> {
        ioctl::kvm_run(&self.cpufd)?;
        Ok(())
    }

    pub fn get_vcpu_mmap_size(&self) -> Result<usize> {
        Ok(ioctl::kvm_get_vcpu_mmap_size(&self.sysfd)? as usize)
    }
}


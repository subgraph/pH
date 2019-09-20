use std::os::unix::io::RawFd;
use std::sync::Arc;

mod ioctl;

use crate::vm::{Result,Error,ErrorKind};
pub use self::ioctl::{KvmCpuIdEntry,KvmLapicState, KvmSRegs, KvmRegs, KvmFpu, KvmMsrs, KvmSegment};

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
    vcpus: Vec<KvmVcpu>,
}

fn check_extensions(sysfd: &ioctl::SysFd, extensions: &[u32]) -> Result<()> {
    for e in extensions {
        if ioctl::kvm_check_extension(&sysfd, *e)? == 0 {
            return Err(Error::from(ErrorKind::MissingRequiredExtension(*e)));
        }
    }
    Ok(())
}

fn check_version(sysfd: &ioctl::SysFd) -> Result<()> {
    if ioctl::kvm_get_api_version(&sysfd)? != 12 {
        return Err(Error::from(ErrorKind::BadVersion));
    }
    Ok(())
}

impl Kvm {
    pub fn open(required_extensions: &[u32]) -> Result<Kvm> {
        let sysfd = ioctl::SysFd::open()?;

        check_version(&sysfd)?;
        check_extensions(&sysfd, &required_extensions)?;

        let vmfd= ioctl::kvm_create_vm(&sysfd)
            .map_err(|_| Error::from(ErrorKind::CreateVmFailed))?;

        Ok(Kvm{
            sysfd: Arc::new(sysfd),
            vmfd: Arc::new(vmfd),
            vcpus: Vec::new(),
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

    pub fn create_pit2(&self) -> Result<()> {
        let pit_config = ioctl::KvmPitConfig::new(0);
        ioctl::kvm_create_pit2(&self.vmfd, &pit_config)?;
        Ok(())
    }

    pub fn create_irqchip(&self) -> Result<()> {
        ioctl::kvm_create_irqchip(&self.vmfd)?;
        Ok(())
    }

    pub fn set_tss_addr(&self, addr: u32) -> Result<()> {
        ioctl::kvm_set_tss_addr(&self.vmfd, addr)?;
        Ok(())
    }

    pub fn irq_line(&self, irq: u32, level: u32) -> Result<()> {
        let irq_level = ioctl::KvmIrqLevel::new(irq, level);
        ioctl::kvm_irq_line(&self.vmfd, &irq_level)?;
        Ok(())
    }

    pub fn irqfd(&self, fd: u32, gsi: u32) -> Result<()> {
        let irqfd = ioctl::KvmIrqFd::new(fd, gsi);
        ioctl::kvm_irqfd(&self.vmfd, &irqfd)?;
        Ok(())
    }

    pub fn ioeventfd_add(&self, address: u64, fd: RawFd) -> Result<()> {
        // XXX check for zero length capability
        let ioeventfd = ioctl::KvmIoEventFd::new_with_addr_fd(address, fd);
        ioctl::kvm_ioeventfd(&self.vmfd, &ioeventfd)?;
        Ok(())
    }

    pub fn ioeventfd_del(&self, address: u64, fd: RawFd) -> Result<()> {
        let mut ioeventfd = ioctl::KvmIoEventFd::new_with_addr_fd(address, fd);
        ioeventfd.set_deassign();
        ioctl::kvm_ioeventfd(&self.vmfd, &ioeventfd)?;
        Ok(())
    }

    pub fn create_vcpus(&mut self, ncpus: usize) -> Result<()> {
        for id in 0..ncpus {
            let vcpu = self.new_vcpu(id)?;
            vcpu.setup_lapic()?;
            self.vcpus.push(vcpu);
        }
        Ok(())
    }

    fn new_vcpu(&self, id: usize) -> Result<KvmVcpu> {
        let cpufd = ioctl::kvm_create_vcpu(&self.vmfd, id as u32)?;
        Ok(KvmVcpu::new(id, Arc::new(cpufd), self.sysfd.clone()))
    }

    pub fn get_vcpus(&self) -> Vec<KvmVcpu> {
        self.vcpus.clone()
    }
}

#[derive(Clone)]
pub struct KvmVcpu {
    id: usize,
    cpufd: Arc<ioctl::VcpuFd>,
    sysfd: Arc<ioctl::SysFd>,

}

const APIC_MODE_EXTINT: u8 = 0x7;
const APIC_MODE_NMI: u8 = 0x4;
const APIC_LVT_LINT0_OFFSET: usize = 0x350;
const APIC_LVT_LINT1_OFFSET: usize = 0x360;

impl KvmVcpu {
    fn new(id: usize, cpufd: Arc<ioctl::VcpuFd>, sysfd: Arc<ioctl::SysFd>) -> KvmVcpu {
        KvmVcpu { id, cpufd, sysfd }
    }

    pub fn raw_fd(&self) -> RawFd {
        self.cpufd.raw()
    }

    pub fn get_supported_cpuid(&self) -> Result<Vec<KvmCpuIdEntry>> {
        let mut cpuid = ioctl::KvmCpuId2::new();
        ioctl::kvm_get_supported_cpuid(&self.sysfd, &mut cpuid)?;
        Ok(cpuid.get_entries())
    }

    pub fn set_cpuid2(&self, entries: Vec<KvmCpuIdEntry>) -> Result<()> {
        let cpuid = ioctl::KvmCpuId2::new_from_entries(entries);
        ioctl::kvm_set_cpuid2(&self.cpufd, &cpuid)?;
        Ok(())
    }

    pub fn get_lapic(&self) -> Result<KvmLapicState> {
        let mut lapic = KvmLapicState::new();
        ioctl::kvm_get_lapic(&self.cpufd, &mut lapic)?;
        Ok(lapic)
    }

    pub fn set_lapic(&self, lapic_state: &KvmLapicState) -> Result<()> {
        ioctl::kvm_set_lapic(&self.cpufd, &lapic_state)?;
        Ok(())
    }

    pub fn get_sregs(&self) -> Result<KvmSRegs> {
        let mut sregs = KvmSRegs::new();
        ioctl::kvm_get_sregs(&self.cpufd, &mut sregs)?;
        Ok(sregs)
    }

    pub fn set_sregs(&self, sregs: &KvmSRegs) -> Result<()> {
        ioctl::kvm_set_sregs(&self.cpufd, &sregs)?;
        Ok(())
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

    pub fn set_fpu(&self, fpu: &KvmFpu) -> Result<()> {
        ioctl::kvm_set_fpu(&self.cpufd, &fpu)?;
        Ok(())
    }

    pub fn set_msrs(&self, msrs: &KvmMsrs) -> Result<()> {
        ioctl::kvm_set_msrs(&self.cpufd, &msrs)?;
        Ok(())
    }

    pub fn get_vcpu_mmap_size(&self) -> Result<usize> {
        Ok(ioctl::kvm_get_vcpu_mmap_size(&self.sysfd)? as usize)
    }

    pub fn setup_lapic(&self) -> Result<()> {
        let mut lapic = self.get_lapic()?;
        // delivery mode
        lapic.regs[APIC_LVT_LINT0_OFFSET + 1] &= 0xF8;
        lapic.regs[APIC_LVT_LINT0_OFFSET + 1] |= APIC_MODE_EXTINT;
        lapic.regs[APIC_LVT_LINT1_OFFSET + 1] &= 0xF8;
        lapic.regs[APIC_LVT_LINT1_OFFSET + 1] |= APIC_MODE_NMI;
        self.set_lapic(&lapic)?;
        Ok(())
    }
}


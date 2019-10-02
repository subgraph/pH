use crate::memory::{MemoryManager, GuestRam, SystemAllocator, AddressRange};
use crate::vm::VmConfig;
use crate::vm::arch::{ArchSetup, Error, Result};
use crate::vm::kernel_cmdline::KernelCmdLine;
use crate::virtio::PciIrq;
use crate::kvm::{Kvm, KvmVcpu};
use crate::vm::arch::x86::kvm::x86_open_kvm;
use crate::vm::arch::x86::memory::{x86_setup_memory_regions, x86_setup_memory};
use crate::vm::arch::x86::cpuid::setup_cpuid;
use crate::vm::arch::x86::registers::{setup_pm_sregs, setup_pm_regs, setup_fpu, setup_msrs};
use crate::vm::arch::x86::interrupts::setup_lapic;
use crate::vm::arch::x86::kernel::KVM_KERNEL_LOAD_ADDRESS;

pub struct X86ArchSetup {
    ram_size: usize,
    use_drm: bool,
    ncpus: usize,
    memory: Option<MemoryManager>,
}

impl X86ArchSetup {
    pub fn create(config: &VmConfig) -> Self {
        let ram_size = config.ram_size();
        let use_drm = config.is_wayland_enabled() && config.is_dmabuf_enabled();
        X86ArchSetup {
            ram_size,
            use_drm,
            ncpus: config.ncpus(),
            memory: None,
        }
    }
}

fn get_base_dev_pfn(mem_size: u64) -> u64 {
    // Put device memory at a 2MB boundary after physical memory or 4gb, whichever is greater.
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;
    let mem_size_round_2mb = (mem_size + 2 * MB - 1) / (2 * MB) * (2 * MB);
    std::cmp::max(mem_size_round_2mb, 4 * GB) / 4096
}

impl ArchSetup for X86ArchSetup {
    fn open_kvm(&self) -> Result<Kvm> {
        x86_open_kvm()
    }

    fn create_memory(&mut self, kvm: &Kvm) -> Result<MemoryManager> {
        let ram = GuestRam::new(self.ram_size);
        let dev_addr_start = get_base_dev_pfn(self.ram_size as u64) * 4096;
        let dev_addr_size = u64::max_value() - dev_addr_start;
        let allocator = SystemAllocator::new(AddressRange::new(dev_addr_start,dev_addr_size as usize));
        let mut mm = MemoryManager::new(kvm.clone(), ram, allocator, self.use_drm)
            .map_err(Error::MemoryManagerCreate)?;
        x86_setup_memory_regions(&mut mm, self.ram_size)?;
        self.memory = Some(mm.clone());
        Ok(mm)
    }

    fn setup_memory(&mut self, cmdline: &KernelCmdLine, pci_irqs: &[PciIrq]) -> Result<()> {
        let memory = self.memory.as_mut().expect("No memory created");
        x86_setup_memory(memory, cmdline, self.ncpus, pci_irqs)?;
        Ok(())
    }

    fn setup_vcpu(&self, vcpu: &KvmVcpu) -> Result<()> {
        setup_cpuid(vcpu)?;
        setup_pm_sregs(vcpu)?;
        setup_pm_regs(&vcpu, KVM_KERNEL_LOAD_ADDRESS)?;
        setup_fpu(vcpu)?;
        setup_msrs(vcpu)?;
        setup_lapic(vcpu.raw_fd())
    }
}

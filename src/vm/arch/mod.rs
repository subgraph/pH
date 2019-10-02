use crate::kvm::{KvmVcpu, Kvm};
pub use crate::vm::arch::x86::X86ArchSetup;
use crate::memory::MemoryManager;

mod error;
mod x86;

pub use x86::PCI_MMIO_RESERVED_BASE;

pub use x86::KvmRegs;
pub use error::{Error,Result};
use crate::vm::kernel_cmdline::KernelCmdLine;
use crate::vm::VmConfig;
use crate::virtio::PciIrq;

pub fn create_setup(config: &VmConfig) -> X86ArchSetup {
    X86ArchSetup::create(config)
}

pub trait ArchSetup {
    fn open_kvm(&self) -> Result<Kvm>;
    fn create_memory(&mut self, kvm: &Kvm) -> Result<MemoryManager>;
    fn setup_memory(&mut self, cmdline: &KernelCmdLine, pci_irqs: &[PciIrq]) -> Result<()>;
    fn setup_vcpu(&self, vcpu: &KvmVcpu) -> Result<()>;
}



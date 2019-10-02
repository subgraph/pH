use std::os::unix::io::RawFd;
use crate::kvm::{Kvm, KVM_CAP_IOEVENTFD, KVM_CAP_PIT2, KVM_CAP_IRQ_INJECT_STATUS, KVM_CAP_IRQ_ROUTING, KVM_CAP_EXT_CPUID, KVM_CAP_SET_TSS_ADDR, KVM_CAP_USER_MEMORY, KVM_CAP_HLT, KVM_CAP_IRQCHIP};
use crate::vm::arch::{Result,Error};

use libc::c_ulong;
use crate::vm::arch::x86::ioctl::{
    call_ioctl_with_ref, call_ioctl_with_val, KVM_CREATE_PIT2, KVM_SET_TSS_ADDR
};

static REQUIRED_EXTENSIONS: &[u32] = &[
    KVM_CAP_IRQCHIP,
    KVM_CAP_HLT,
    KVM_CAP_USER_MEMORY,
    KVM_CAP_SET_TSS_ADDR,
    KVM_CAP_EXT_CPUID,
    KVM_CAP_IRQ_ROUTING,
    KVM_CAP_IRQ_INJECT_STATUS,
    KVM_CAP_PIT2,
    KVM_CAP_IOEVENTFD,
];

pub fn x86_open_kvm() -> Result<Kvm> {
    let kvm = Kvm::open(REQUIRED_EXTENSIONS)
        .map_err(Error::KvmError)?;
    kvm.create_irqchip().map_err(Error::KvmError)?;
    kvm_set_tss_addr(kvm.vmfd(), 0xFFFbd000)?;
    kvm_create_pit2(kvm.vmfd())?;
    Ok(kvm)
}

#[repr(C)]
struct KvmPitConfig {
    flags: u32,
    padding: [u32; 15],
}

impl KvmPitConfig {
    pub fn new(flags: u32) -> KvmPitConfig {
        KvmPitConfig { flags, padding: [0; 15] }
    }
}

fn kvm_create_pit2(vmfd: RawFd) -> Result<()> {
    let pit_config = KvmPitConfig::new(0);
    call_ioctl_with_ref("KVM_CREATE_PIT2", vmfd, KVM_CREATE_PIT2, &pit_config)
}

fn kvm_set_tss_addr(vmfd: RawFd, addr: u32) -> Result<()> {
    call_ioctl_with_val("KVM_SET_TSS_ADDR", vmfd, KVM_SET_TSS_ADDR, addr as c_ulong)
}

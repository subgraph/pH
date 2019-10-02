use std::os::unix::io::RawFd;

use crate::system::ioctl::{ioctl_with_mut_ref, ioctl_with_ref};
use crate::vm::arch::{Error,Result};
use crate::vm::arch::x86::ioctl::{KVM_GET_LAPIC, KVM_SET_LAPIC};

#[repr(C)]
pub struct KvmLapicState {
    pub regs: [u8; 1024]
}

impl KvmLapicState {
    pub fn new() -> KvmLapicState {
        KvmLapicState { regs: [0; 1024] }
    }
}

pub fn kvm_get_lapic(cpufd: RawFd) -> Result<KvmLapicState> {
    let mut lapic_state = KvmLapicState::new();
    unsafe {
        ioctl_with_mut_ref(cpufd, KVM_GET_LAPIC, &mut lapic_state)
            .map_err(|e| Error::IoctlError("KVM_GET_LAPIC", e))?;
    }
    Ok(lapic_state)
}

pub fn kvm_set_lapic(cpufd: RawFd, lapic_state: &KvmLapicState) -> Result<()> {
    unsafe {
        ioctl_with_ref(cpufd, KVM_SET_LAPIC, lapic_state)
            .map_err(|e| Error::IoctlError("KVM_SET_LAPIC", e))?;
    }
    Ok(())
}

const APIC_MODE_EXTINT: u8 = 0x7;
const APIC_MODE_NMI: u8 = 0x4;
const APIC_LVT_LINT0_OFFSET: usize = 0x350;
const APIC_LVT_LINT1_OFFSET: usize = 0x360;

pub fn setup_lapic(cpufd: RawFd) -> Result<()> {
    let mut lapic = kvm_get_lapic(cpufd)?;
    // delivery mode
    lapic.regs[APIC_LVT_LINT0_OFFSET + 1] &= 0xF8;
    lapic.regs[APIC_LVT_LINT0_OFFSET + 1] |= APIC_MODE_EXTINT;
    lapic.regs[APIC_LVT_LINT1_OFFSET + 1] &= 0xF8;
    lapic.regs[APIC_LVT_LINT1_OFFSET + 1] |= APIC_MODE_NMI;
    kvm_set_lapic(cpufd, &lapic)
}



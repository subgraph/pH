
use std::os::unix::io::RawFd;
use crate::vm::arch::Result;
use crate::kvm::KvmVcpu;
use crate::vm::arch::x86::ioctl::{KVM_GET_SUPPORTED_CPUID, KVM_SET_CPUID2, call_ioctl_with_ref, call_ioctl_with_mut_ref};

const EBX_CLFLUSH_CACHELINE: u32 = 8; // Flush a cache line size.
const EBX_CLFLUSH_SIZE_SHIFT: u32 = 8; // Bytes flushed when executing CLFLUSH.
const _EBX_CPU_COUNT_SHIFT: u32 = 16; // Index of this CPU.
const EBX_CPUID_SHIFT: u32 = 24; // Index of this CPU.
const _ECX_EPB_SHIFT: u32 = 3; // "Energy Performance Bias" bit.
const _ECX_HYPERVISOR_SHIFT: u32 = 31; // Flag to be set when the cpu is running on a hypervisor.
const _EDX_HTT_SHIFT: u32 = 28; // Hyper Threading Enabled.

pub fn setup_cpuid(vcpu: &KvmVcpu) -> Result<()> {
    let mut cpuid = kvm_get_supported_cpuid(vcpu.sys_raw_fd())?;
    let cpu_id = 0u32; // first vcpu

    for e in &mut cpuid {
        match e.function {
            0 => {
                e.ebx = 0x67627553;
                e.ecx = 0x20487020;
                e.edx = 0x68706172;
            }
            1 => {
                if e.index == 0 {
                    e.ecx |= 1<<31;
                }
                e.ebx = (cpu_id << EBX_CPUID_SHIFT) as u32 |
                    (EBX_CLFLUSH_CACHELINE << EBX_CLFLUSH_SIZE_SHIFT);
                /*
                if cpu_count > 1 {
                    entry.ebx |= (cpu_count as u32) << EBX_CPU_COUNT_SHIFT;
                    entry.edx |= 1 << EDX_HTT_SHIFT;
                }
                */
            }
            6 => {
                e.ecx &= !(1<<3);

            }
            10 => {
                if e.eax > 0 {
                    let version = e.eax & 0xFF;
                    let ncounters = (e.eax >> 8) & 0xFF;
                    if version != 2 || ncounters == 0 {
                        e.eax = 0;
                    }
                }

            }
            _ => {}
        }
    }
    kvm_set_cpuid2(vcpu.raw_fd(), cpuid)
}


pub fn kvm_get_supported_cpuid(sysfd: RawFd) -> Result<Vec<KvmCpuIdEntry>> {
    let mut cpuid = KvmCpuId2::new();
    call_ioctl_with_mut_ref("KVM_GET_SUPPORTED_CPUID", sysfd, KVM_GET_SUPPORTED_CPUID, &mut cpuid)?;
    Ok(cpuid.get_entries())
}

pub fn kvm_set_cpuid2(cpufd: RawFd, entries: Vec<KvmCpuIdEntry>) -> Result<()> {
    let cpuid = KvmCpuId2::new_from_entries(entries);
    call_ioctl_with_ref("KVM_SET_CPUID2", cpufd, KVM_SET_CPUID2, &cpuid)
}

#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct KvmCpuIdEntry {
    pub function: u32,
    pub index: u32,
    pub flags: u32,
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    padding: [u32; 3]
}

const KVM_CPUID_MAX_ENTRIES:usize = 256;

#[repr(C)]
pub struct KvmCpuId2 {
    nent: u32,
    padding: u32,
    entries: [KvmCpuIdEntry; KVM_CPUID_MAX_ENTRIES]
}

impl KvmCpuId2 {
    pub fn new() -> KvmCpuId2 {
        KvmCpuId2 {
            nent: KVM_CPUID_MAX_ENTRIES as u32,
            padding: 0,
            entries: [Default::default(); KVM_CPUID_MAX_ENTRIES],
        }
    }

    pub fn new_from_entries(entries: Vec<KvmCpuIdEntry>) -> KvmCpuId2 {
        let mut cpuid = KvmCpuId2::new();
        let sz = entries.len();
        assert!(sz <= KVM_CPUID_MAX_ENTRIES, "Too many cpuid entries");
        for i in 0..sz {
            cpuid.entries[i] = entries[i];
        }
        cpuid.nent = sz as u32;
        cpuid
    }

    pub fn get_entries(&self) -> Vec<KvmCpuIdEntry> {
        let mut entries = Vec::new();
        let sz = self.nent as usize;
        for i in 0..sz {
            entries.push(self.entries[i]);
        }
        entries
    }
}

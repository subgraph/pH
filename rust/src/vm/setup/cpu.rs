use crate::vm::Result;

use crate::kvm::{KvmVcpu,KvmRegs,KvmFpu, KvmMsrs, KvmSegment};
use crate::memory::{GuestRam,KERNEL_ZERO_PAGE};


const MSR_IA32_SYSENTER_CS: u32  = 0x00000174;
const MSR_IA32_SYSENTER_ESP: u32 = 0x00000175;
const MSR_IA32_SYSENTER_EIP: u32 = 0x00000176;
const MSR_STAR: u32              = 0xc0000081;
const MSR_LSTAR: u32             = 0xc0000082;
const MSR_CSTAR: u32             = 0xc0000083;
const MSR_SYSCALL_MASK: u32      = 0xc0000084;
const MSR_KERNEL_GS_BASE: u32    = 0xc0000102;
const MSR_IA32_TSC: u32          = 0x00000010;
const MSR_IA32_MISC_ENABLE: u32  = 0x000001a0;

const MSR_IA32_MISC_ENABLE_FAST_STRING: u64 = 0x01;


const EBX_CLFLUSH_CACHELINE: u32 = 8; // Flush a cache line size.
const EBX_CLFLUSH_SIZE_SHIFT: u32 = 8; // Bytes flushed when executing CLFLUSH.
const _EBX_CPU_COUNT_SHIFT: u32 = 16; // Index of this CPU.
const EBX_CPUID_SHIFT: u32 = 24; // Index of this CPU.
const _ECX_EPB_SHIFT: u32 = 3; // "Energy Performance Bias" bit.
const _ECX_HYPERVISOR_SHIFT: u32 = 31; // Flag to be set when the cpu is running on a hypervisor.
const _EDX_HTT_SHIFT: u32 = 28; // Hyper Threading Enabled.

fn setup_cpuid(vcpu: &KvmVcpu) -> Result<()> {
    let mut cpuid = vcpu.get_supported_cpuid()?;
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
    vcpu.set_cpuid2(cpuid)?;
    Ok(())
}

fn setup_fpu(vcpu: &KvmVcpu) -> Result<()> {
    let mut fpu = KvmFpu::new();
    fpu.fcw = 0x37f;
    fpu.mxcsr = 0x1f80;
    vcpu.set_fpu(&fpu)?;
    Ok(())
}

fn setup_msrs(vcpu: &KvmVcpu) -> Result<()> {
    let mut msrs = KvmMsrs::new();
    msrs.add(MSR_IA32_SYSENTER_CS, 0);
    msrs.add(MSR_IA32_SYSENTER_ESP, 0);
    msrs.add(MSR_IA32_SYSENTER_EIP, 0);
    msrs.add(MSR_STAR, 0);
    msrs.add(MSR_CSTAR, 0);
    msrs.add(MSR_KERNEL_GS_BASE, 0);
    msrs.add(MSR_SYSCALL_MASK, 0);
    msrs.add(MSR_LSTAR, 0);
    msrs.add(MSR_IA32_TSC, 0);
    msrs.add(MSR_IA32_MISC_ENABLE, MSR_IA32_MISC_ENABLE_FAST_STRING);
    vcpu.set_msrs(&msrs)?;
    Ok(())
}


pub fn gdt_entry(flags: u16, base: u32, limit: u32) -> u64 {
    ((((base as u64) & 0xff000000u64) << (56 - 24)) | (((flags as u64) & 0x0000f0ffu64) << 40) |
        (((limit as u64) & 0x000f0000u64) << (48 - 16)) |
        (((base as u64) & 0x00ffffffu64) << 16) | ((limit as u64) & 0x0000ffffu64))
}
const BOOT_GDT_OFFSET: usize = 0x500;
const BOOT_IDT_OFFSET: usize = 0x520;

const BOOT_STACK: u64 = 0x8000;
const BOOT_PML4: u64 = 0x9000;
const BOOT_PDPTE: u64 = 0xA000;
const BOOT_PDE: u64 = 0xB000;


const X86_CR0_PE: u64 = 0x1;
const X86_CR0_PG: u64 = 0x80000000;
const X86_CR4_PAE: u64 = 0x20;

const EFER_LME: u64 = 0x100;
const EFER_LMA: u64 = (1 << 10);

fn setup_boot_pagetables(memory: &GuestRam) -> Result<()> {
    memory.write_int::<u64>(BOOT_PML4, BOOT_PDPTE | 0x3)?;
    memory.write_int::<u64>(BOOT_PDPTE, BOOT_PDE | 0x3)?;
    for i in 0..512_u64 {
        let entry = (i << 21) | 0x83;
        memory.write_int::<u64>(BOOT_PDE + (i * 8), entry)?;
    }
    Ok(())
}

fn write_gdt_table(table: &[u64], memory: &GuestRam) -> Result<()> {
    for i in 0..table.len() {
        memory.write_int((BOOT_GDT_OFFSET + i * 8) as u64, table[i])?;
    }
    Ok(())
}

pub fn setup_pm_sregs(vcpu: &KvmVcpu, memory: &GuestRam) -> Result<()> {
    let table = [
        gdt_entry(0,0,0),
        gdt_entry(0xa09b,0,0xfffff),
        gdt_entry(0xc093,0,0xfffff),
        gdt_entry(0x808b,0,0xfffff),
    ];
    write_gdt_table(&table, memory)?;

    memory.write_int::<u64>(BOOT_IDT_OFFSET as u64, 0u64)?;

    let code = KvmSegment::new(0, 0xfffff, 1 * 8, 0xa09b);
    let data = KvmSegment::new(0, 0xfffff, 2 * 8, 0xc093);
    let tss = KvmSegment::new(0, 0xfffff, 3 * 8, 0x808b);

    let mut regs = vcpu.get_sregs()?;

    regs.gdt.base = BOOT_GDT_OFFSET as u64;
    regs.gdt.limit = 32 - 1;

    regs.itd.base = BOOT_IDT_OFFSET as u64;
    regs.itd.limit = 8 - 1;

    regs.cs = code;
    regs.ds = data;
    regs.es = data;
    regs.fs = data;
    regs.gs = data;
    regs.ss = data;
    regs.tr = tss;

    // protected mode
    regs.cr0 |= X86_CR0_PE;
    regs.efer |= EFER_LME;

    setup_boot_pagetables(&memory)?;
    regs.cr3 = BOOT_PML4;
    regs.cr4 |= X86_CR4_PAE;
    regs.cr0 |= X86_CR0_PG;
    regs.efer |= EFER_LMA;

    vcpu.set_sregs(&regs)?;
    Ok(())
}

pub fn setup_pm_regs(vcpu: &KvmVcpu, kernel_entry: u64) -> Result<()> {
    let mut regs = KvmRegs::new();
    regs.rflags = 0x0000000000000002;
    regs.rip = kernel_entry;
    regs.rsp = BOOT_STACK;
    regs.rbp = BOOT_STACK;
    regs.rsi = KERNEL_ZERO_PAGE;
    vcpu.set_regs(&regs)?;
    Ok(())
}

pub fn setup_protected_mode(vcpu: &KvmVcpu, kernel_entry: u64, memory: &GuestRam) -> Result<()> {
    setup_cpuid(&vcpu)?;
    setup_pm_sregs(&vcpu, memory)?;
    setup_pm_regs(&vcpu, kernel_entry)?;
    setup_fpu(&vcpu)?;
    setup_msrs(&vcpu)?;
    Ok(())
}

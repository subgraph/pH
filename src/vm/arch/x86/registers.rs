use std::fmt;
use std::os::unix::io::RawFd;

use crate::kvm::KvmVcpu;
use crate::vm::arch::{Result, Error};
use crate::vm::arch::x86::kernel::KERNEL_ZERO_PAGE;
use crate::vm::arch::x86::ioctl::{
    call_ioctl_with_ref, KVM_SET_FPU, KVM_SET_MSRS, call_ioctl_with_mut_ref, KVM_GET_SREGS, KVM_SET_SREGS
};

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

pub fn setup_fpu(vcpu: &KvmVcpu) -> Result<()> {
    let mut fpu = KvmFpu::new();
    fpu.fcw = 0x37f;
    fpu.mxcsr = 0x1f80;
    kvm_set_fpu(vcpu.raw_fd(), &fpu)?;
    Ok(())
}

pub fn setup_msrs(vcpu: &KvmVcpu) -> Result<()> {
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
    kvm_set_msrs(vcpu.raw_fd(), &msrs)?;
    Ok(())
}

const BOOT_GDT_OFFSET: usize = 0x500;
const BOOT_IDT_OFFSET: usize = 0x520;

const BOOT_STACK: u64 = 0x8000;
const BOOT_PML4: u64 = 0x9000;

const X86_CR0_PE: u64 = 0x1;
const X86_CR0_PG: u64 = 0x80000000;
const X86_CR4_PAE: u64 = 0x20;

const EFER_LME: u64 = 0x100;
const EFER_LMA: u64 = (1 << 10);

pub fn setup_pm_sregs(vcpu: &KvmVcpu) -> Result<()> {

    let code = KvmSegment::new(0, 0xfffff, 1 * 8, 0xa09b);
    let data = KvmSegment::new(0, 0xfffff, 2 * 8, 0xc093);
    let tss = KvmSegment::new(0, 0xfffff, 3 * 8, 0x808b);

    let mut regs = kvm_get_sregs(vcpu.raw_fd())?;

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

    regs.cr3 = BOOT_PML4;
    regs.cr4 |= X86_CR4_PAE;
    regs.cr0 |= X86_CR0_PG;
    regs.efer |= EFER_LMA;

    kvm_set_sregs(vcpu.raw_fd(), &regs)?;
    Ok(())
}

pub fn setup_pm_regs(vcpu: &KvmVcpu, kernel_entry: u64) -> Result<()> {
    let mut regs = KvmRegs::new();
    regs.rflags = 0x0000000000000002;
    regs.rip = kernel_entry;
    regs.rsp = BOOT_STACK;
    regs.rbp = BOOT_STACK;
    regs.rsi = KERNEL_ZERO_PAGE;
    vcpu.set_regs(&regs)
        .map_err(Error::KvmError)?;
    Ok(())
}

#[derive(Copy)]
#[repr(C)]
pub struct KvmFpu {
    fpr: [u8; 128],
    pub fcw: u16,
    fsw: u16,
    ftwx: u8,
    pad1: u8,
    last_opcode: u16,
    last_ip: u64,
    last_dp: u64,
    xmm: [u8; 256],
    pub mxcsr: u32,
    pad2: u32,
}

impl Clone for KvmFpu {
    fn clone(&self) -> KvmFpu { *self }
}
impl KvmFpu {
    pub fn new() -> KvmFpu {
        KvmFpu {
            fpr: [0; 128],
            fcw: 0,
            fsw: 0,
            ftwx: 0, pad1: 0,
            last_opcode: 0,
            last_ip: 0,
            last_dp: 0,
            xmm: [0; 256],
            mxcsr: 0,
            pad2: 0
        }
    }
}

pub fn kvm_set_fpu(cpufd: RawFd, fpu: &KvmFpu) -> Result<()> {
    call_ioctl_with_ref("KVM_SET_FPU", cpufd, KVM_SET_FPU, fpu)
}

#[derive(Copy, Clone, Default)]
#[repr(C)]
struct KvmMsrEntry {
    index: u32,
    reserved: u32,
    data: u64
}

#[repr(C)]
pub struct KvmMsrs {
    nent: u32,
    padding: u32,
    entries: [KvmMsrEntry; 100]
}

impl KvmMsrs {
    pub fn new() -> KvmMsrs {
        KvmMsrs{ nent: 0, padding: 0, entries: [Default::default(); 100]}
    }

    pub fn add(&mut self, index: u32, data: u64) {
        self.entries[self.nent as usize].index = index;
        self.entries[self.nent as usize].data = data;
        self.nent += 1;
    }
}

pub fn kvm_set_msrs(cpufd: RawFd, msrs: &KvmMsrs) -> Result<()> {
    call_ioctl_with_ref("KVM_SET_MSRS", cpufd, KVM_SET_MSRS, msrs)
}

#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct KvmSegment {
    base: u64,
    limit: u32,
    selector: u16,
    stype: u8,
    present: u8,
    dpl: u8,
    db: u8,
    s: u8,
    l: u8,
    g: u8,
    avl: u8,
    unusable: u8,
    padding: u8,
}

impl KvmSegment {
    pub fn new(base: u64, limit: u32, selector: u16, flags: u16) -> KvmSegment {
        let mut seg = KvmSegment{ ..Default::default() };
        seg.setup(base, limit, selector, flags);
        seg
    }

    pub fn setup(&mut self, base: u64, limit: u32, selector: u16, flags: u16) {
        self.base = base;
        self.limit = limit;
        self.selector = selector;
        self.stype = (flags & 0xF) as u8;
        self.present = ((flags >> 7) & 0x1) as u8;
        self.dpl = ((flags >> 5) & 0x3) as u8;
        self.db = ((flags >> 14) & 0x1) as u8;
        self.s = ((flags >> 4) & 0x1) as u8;
        self.l = ((flags >> 13) & 0x1) as u8;
        self.g = ((flags >> 15) & 0x1) as u8;
        self.avl = ((flags >> 12) & 0x1) as u8;
        self.unusable = if self.present == 1 { 0 } else { 1 }
    }
}

impl fmt::Debug for KvmSegment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(base: {:x} limit {:x} selector: {:x} type: {:x} p: {} dpl: {} db: {} s: {} l: {} g: {} avl: {} unuse: {})",
               self.base, self.limit, self.selector, self.stype, self.present, self.dpl, self.db, self.s, self.l, self.g, self.avl, self.unusable)
    }
}

#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct KvmDtable {
    pub base: u64,
    pub limit: u16,
    padding: [u16; 3],
}

impl fmt::Debug for KvmDtable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(base: {:x} limit {:x})", self.base, self.limit)
    }
}

#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct KvmSRegs {
    pub cs: KvmSegment,
    pub ds: KvmSegment,
    pub es: KvmSegment,
    pub fs: KvmSegment,
    pub gs: KvmSegment,
    pub ss: KvmSegment,
    pub tr: KvmSegment,
    pub ldt: KvmSegment,
    pub gdt: KvmDtable,
    pub itd: KvmDtable,
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,
    pub efer: u64,
    pub apic_base: u64,
    pub interrupt_bitmap: [u64; 4],
}

impl fmt::Debug for KvmSRegs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "cs: {:?}\nds: {:?}\nes: {:?}\nfs: {:?}\n", self.cs, self.ds, self.es, self.fs)?;
        write!(f, "gs: {:?}\nss: {:?}\ntr: {:?}\nldt: {:?}\n", self.gs, self.ss, self.tr, self.ldt)?;
        write!(f, "gdt: {:?} itd: {:?}\n", self.gdt, self.itd)?;
        write!(f, "cr0: {:x} cr2: {:x} cr3: {:x} cr4: {:x}\n", self.cr0, self.cr2, self.cr3, self.cr4)?;
        write!(f, "efer: {:x} apic_base: {:x}\n", self.efer, self.apic_base)
    }
}

impl KvmSRegs {
    pub fn new() -> KvmSRegs {
        KvmSRegs { ..Default::default() }
    }
}

pub fn kvm_get_sregs(cpufd: RawFd) -> Result<KvmSRegs> {
    let mut sregs = KvmSRegs::new();
    call_ioctl_with_mut_ref("KVM_GET_SREGS", cpufd, KVM_GET_SREGS, &mut sregs)?;
    Ok(sregs)
}

pub fn kvm_set_sregs(cpufd: RawFd, sregs: &KvmSRegs) -> Result<()> {
    call_ioctl_with_ref("KVM_SET_SREGS", cpufd, KVM_SET_SREGS, sregs)
}

#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct KvmRegs {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rsi: u64, pub rdi: u64, pub rsp: u64, pub rbp: u64,
    pub r8: u64, pub r9: u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
    pub rip: u64, pub rflags: u64,
}

impl fmt::Debug for KvmRegs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rax 0x{:x} rbx 0x{:x} rcx 0x{:x} rdx 0x{:x}\n", self.rax, self.rbx, self.rcx, self.rdx)?;
        write!(f, "rsi 0x{:x} rdi 0x{:x} rsp 0x{:x} rbp 0x{:x}\n", self.rsi, self.rdi, self.rsp, self.rbp)?;
        write!(f, "r8  0x{:x} r9  0x{:x} r10 0x{:x} r11 0x{:x}\n", self.r8, self.r9, self.r10, self.r11)?;
        write!(f, "r12 0x{:x} r13 0x{:x} r14 0x{:x} r15 0x{:x}\n", self.r12, self.r13, self.r14, self.r15)?;
        write!(f, "rip 0x{:x} rflags 0x{:x}\n", self.rip, self.rflags)
    }
}

impl KvmRegs {
    pub fn new() -> KvmRegs {
        KvmRegs { ..Default::default() }
    }
}

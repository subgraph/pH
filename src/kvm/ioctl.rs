use libc::{self, c_char, c_ulong};
use std::os::unix::io::RawFd;
use std::ffi::CString;
use std::fmt;

use crate::system::ioctl::{ioctl_with_val,ioctl_with_ref,ioctl_with_mut_ref};

use crate::vm::{Result,Error,ErrorKind};
use crate::system;


const KVMIO:     u64 = 0xAE;

const KVM_GET_API_VERSION: c_ulong           = io!     (KVMIO, 0x00);
const KVM_CREATE_VM: c_ulong                 = io!     (KVMIO, 0x01);
const KVM_CHECK_EXTENSION: c_ulong           = io!     (KVMIO, 0x03);
const KVM_GET_SUPPORTED_CPUID: c_ulong       = iorw!   (KVMIO, 0x05, 8);
const KVM_SET_TSS_ADDR: c_ulong              = io!     (KVMIO, 0x47);
const KVM_CREATE_IRQCHIP: c_ulong            = io!     (KVMIO, 0x60);
const KVM_CREATE_PIT2: c_ulong               = iow!    (KVMIO, 0x77, 64);
const KVM_GET_VCPU_MMAP_SIZE: c_ulong        = io!     (KVMIO, 0x04);
const KVM_CREATE_VCPU: c_ulong               = io!     (KVMIO, 0x41);
const KVM_SET_USER_MEMORY_REGION: c_ulong    = iow!    (KVMIO, 0x46, 32);
const KVM_IRQ_LINE: c_ulong                  = iow!    (KVMIO, 0x61, 8);
const KVM_IRQFD: c_ulong                     = iow!    (KVMIO, 0x76, 32);
const KVM_IOEVENTFD: c_ulong                 = iow!    (KVMIO, 0x79, 64);
const KVM_RUN: c_ulong                       = io!     (KVMIO, 0x80);
const KVM_GET_REGS: c_ulong                  = ior!    (KVMIO, 0x81, 144);
const KVM_SET_REGS: c_ulong                  = iow!    (KVMIO, 0x82, 144);
const KVM_GET_SREGS: c_ulong                 = ior!    (KVMIO, 0x83, 312);
const KVM_SET_SREGS: c_ulong                 = iow!    (KVMIO, 0x84, 312);
const KVM_SET_MSRS: c_ulong                  = iow!    (KVMIO, 0x89, 8);
const KVM_SET_FPU: c_ulong                   = iow!    (KVMIO, 0x8d, 416);
const KVM_GET_LAPIC: c_ulong                 = ior!    (KVMIO, 0x8e, 1024);
const KVM_SET_LAPIC: c_ulong                 = iow!    (KVMIO, 0x8f, 1024);
const KVM_SET_CPUID2: c_ulong                = iow!    (KVMIO, 0x90, 8);


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
        return Err(Error::from_last_errno());
    }
    Ok(fd)
}

impl SysFd {
    pub fn open() -> Result<SysFd> {
        match raw_open_kvm() {
            Ok(fd) => Ok(SysFd(InnerFd(fd))),
            Err(e) => Err(Error::new(ErrorKind::OpenDeviceFailed, e)),
        }
    }

    fn raw(&self) -> RawFd { self.0.raw() }
}

pub struct VmFd(InnerFd);

impl VmFd {
    fn new(fd: RawFd) -> VmFd {
        VmFd( InnerFd(fd) )
    }
    fn raw(&self) -> RawFd { self.0.raw() }
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
            .map_err(|e| ioctl_err("KVM_CHECK_EXTENSION", e))
    }
}

pub fn kvm_get_api_version(sysfd: &SysFd) -> Result<u32> {
    unsafe {
        ioctl_with_val(sysfd.raw(), KVM_GET_API_VERSION, 0)
            .map_err(|e| ioctl_err("KVM_GET_API_VERSION", e))
    }
}

pub fn kvm_create_vm(sysfd: &SysFd) -> Result<VmFd> {
    let fd = unsafe {
        ioctl_with_val(sysfd.raw(), KVM_CREATE_VM, 0)
            .map_err(|e| ioctl_err("KVM_CREATE_VM", e))?
    };
    Ok(VmFd::new(fd as RawFd))
}

pub fn kvm_get_vcpu_mmap_size(sysfd: &SysFd) -> Result<u32> {
    unsafe {
        ioctl_with_val(sysfd.raw(), KVM_GET_VCPU_MMAP_SIZE, 0)
            .map_err(|e| ioctl_err("KVM_GET_VCPU_MMAP_SIZE", e))
    }
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

pub fn kvm_get_supported_cpuid(sysfd: &SysFd, cpuid: &mut KvmCpuId2) -> Result<u32> {
    unsafe {
        ioctl_with_mut_ref(sysfd.raw(), KVM_GET_SUPPORTED_CPUID, cpuid)
            .map_err(|e| ioctl_err("KVM_GET_SUPPORTED_CPUID", e))
    }
}

pub fn kvm_set_cpuid2(cpufd: &VcpuFd, cpuid: &KvmCpuId2) -> Result<u32> {
    unsafe {
        ioctl_with_ref(cpufd.raw(), KVM_SET_CPUID2, cpuid)
            .map_err(|e| ioctl_err("KVM_SET_CPUID2", e))
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

pub fn kvm_set_user_memory_region(vmfd: &VmFd, region: &KvmUserspaceMemoryRegion) -> Result<u32> {
    unsafe {
        ioctl_with_ref(vmfd.raw(), KVM_SET_USER_MEMORY_REGION, region)
            .map_err(|e| ioctl_err("KVM_SET_USER_MEMORY_REGION", e))
    }
}

#[repr(C)]
pub struct KvmPitConfig {
    flags: u32,
    padding: [u32; 15],
}

impl KvmPitConfig {
    pub fn new(flags: u32) -> KvmPitConfig {
        KvmPitConfig { flags, padding: [0; 15] }
    }
}

pub fn kvm_create_pit2(vmfd: &VmFd, config: &KvmPitConfig) -> Result<u32> {
    unsafe {
        ioctl_with_ref(vmfd.raw(), KVM_CREATE_PIT2, config)
            .map_err(|e| ioctl_err("KVM_CREATE_PIT2", e))
    }
}

pub fn kvm_create_irqchip(vmfd: &VmFd) -> Result<u32> {
    unsafe {
        ioctl_with_val(vmfd.raw(), KVM_CREATE_IRQCHIP, 0)
            .map_err(|e| ioctl_err("KVM_CREATE_IRQCHIP", e))
    }
}

pub fn kvm_set_tss_addr(vmfd: &VmFd, addr: u32) -> Result<u32> {
    unsafe {
        ioctl_with_val(vmfd.raw(), KVM_SET_TSS_ADDR, addr as c_ulong)
            .map_err(|e| ioctl_err("KVM_SET_TSS_ADDR", e))
    }
}

pub fn kvm_create_vcpu(vmfd: &VmFd, cpu_id: u32) -> Result<VcpuFd> {
    let fd = unsafe {
        ioctl_with_val(vmfd.raw(), KVM_CREATE_VCPU, cpu_id as c_ulong)
            .map_err(|e| ioctl_err("KVM_CREATE_VCPU", e))?
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

pub fn kvm_irq_line(vmfd: &VmFd, level: &KvmIrqLevel) -> Result<u32> {
    unsafe {
        ioctl_with_ref(vmfd.raw(), KVM_IRQ_LINE, level)
            .map_err(|e| ioctl_err("KVM_IRQ_LINE", e))
    }
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

pub fn kvm_irqfd(vmfd: &VmFd, irqfd: &KvmIrqFd) -> Result<u32> {
    unsafe {
        ioctl_with_ref(vmfd.raw(), KVM_IRQFD, irqfd)
            .map_err(|e| ioctl_err("KVM_IRQFD", e))
    }
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

pub fn kvm_ioeventfd(vmfd: &VmFd, ioeventfd: &KvmIoEventFd) -> Result<u32> {
    unsafe {
        ioctl_with_ref(vmfd.raw(), KVM_IOEVENTFD, ioeventfd)
            .map_err(|e| ioctl_err("KVM_IOEVENTFD", e))
    }
}


#[repr(C)]
pub struct KvmLapicState {
    pub regs: [u8; 1024]
}

impl KvmLapicState {
    pub fn new() -> KvmLapicState {
        KvmLapicState { regs: [0; 1024] }
    }
}

pub fn kvm_get_lapic(cpufd: &VcpuFd, lapic_state: &mut KvmLapicState) -> Result<u32> {
    unsafe {
        ioctl_with_mut_ref(cpufd.raw(), KVM_GET_LAPIC, lapic_state)
            .map_err(|e| ioctl_err("KVM_GET_LAPIC", e))
    }
}

pub fn kvm_set_lapic(cpufd: &VcpuFd, lapic_state: &KvmLapicState) -> Result<u32> {
    unsafe {
        ioctl_with_ref(cpufd.raw(), KVM_SET_LAPIC, lapic_state)
            .map_err(|e| ioctl_err("KVM_SET_LAPIC", e))
    }
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

pub fn kvm_get_sregs(cpufd: &VcpuFd, sregs: &mut KvmSRegs) -> Result<u32> {
    unsafe {
        ioctl_with_mut_ref(cpufd.raw(), KVM_GET_SREGS, sregs)
            .map_err(|e| ioctl_err("KVM_GET_SREGS", e))
    }
}

pub fn kvm_set_sregs(cpufd: &VcpuFd, sregs: &KvmSRegs) -> Result<u32> {
    unsafe {
        ioctl_with_ref(cpufd.raw(), KVM_SET_SREGS, sregs)
            .map_err(|e| ioctl_err("KVM_SET_SREGS", e))
    }
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


pub fn kvm_get_regs(cpufd: &VcpuFd, regs: &mut KvmRegs) -> Result<u32> {
    unsafe {
        ioctl_with_mut_ref(cpufd.raw(), KVM_GET_REGS, regs)
            .map_err(|e| ioctl_err("KVM_GET_REGS", e))
    }
}

pub fn kvm_set_regs(cpufd: &VcpuFd, regs: &KvmRegs) -> Result<u32> {
    unsafe {
        ioctl_with_ref(cpufd.raw(), KVM_SET_REGS, regs)
            .map_err(|e| ioctl_err("KVM_SET_REGS", e))
    }
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

pub fn kvm_set_fpu(cpufd: &VcpuFd, fpu: &KvmFpu) -> Result<u32> {
    unsafe {
        ioctl_with_ref(cpufd.raw(), KVM_SET_FPU, fpu )
            .map_err(|e| ioctl_err("KVM_SET_FPU", e))
    }
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

pub fn kvm_set_msrs(cpufd: &VcpuFd, msrs: &KvmMsrs) -> Result<u32> {
     unsafe {
        ioctl_with_ref(cpufd.raw(), KVM_SET_MSRS, msrs)
            .map_err(|e| ioctl_err("KVM_SET_MSRS", e))
    }
}

pub fn kvm_run(cpufd: &VcpuFd) -> Result<u32> {
    unsafe {
        ioctl_with_val(cpufd.raw(), KVM_RUN, 0)
            .map_err(|e| ioctl_err("KVM_RUN", e))
    }
}

pub fn ioctl_err(ioctl_name: &'static str, e: system::Error) -> Error {
    if e.is_interrupted() {
        Error::new(ErrorKind::Interrupted, e)
    } else {
        Error::new(ErrorKind::IoctlFailed(ioctl_name), e)
    }
}


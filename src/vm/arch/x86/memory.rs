use crate::kvm::Kvm;
use crate::memory::{MemoryManager, MemoryRegion, GuestRam};
use crate::vm::arch::{Error, Result};
use std::cmp;
use crate::vm::kernel_cmdline::KernelCmdLine;
use crate::vm::arch::x86::kernel::{load_pm_kernel, KERNEL_CMDLINE_ADDRESS};
use crate::system;
use crate::vm::arch::x86::mptable::setup_mptable;
use crate::virtio::PciIrq;

pub const HIMEM_BASE: u64 = (1 << 32);
pub const PCI_MMIO_RESERVED_SIZE: usize = (512 << 20);
pub const PCI_MMIO_RESERVED_BASE: u64 = HIMEM_BASE - PCI_MMIO_RESERVED_SIZE as u64;


pub fn x86_setup_memory_regions(memory: &mut MemoryManager, ram_size: usize) -> Result<()> {
    let mut regions = Vec::new();
    let lowmem_sz = cmp::min(ram_size, PCI_MMIO_RESERVED_BASE as usize);
    regions.push(create_region(memory.kvm(),  0, lowmem_sz, 0)?);

    if lowmem_sz < ram_size {
        let himem_sz = ram_size - lowmem_sz;
        regions.push(create_region(memory.kvm(), HIMEM_BASE, himem_sz, 1)?);
    }
    memory.set_ram_regions(regions);
    Ok(())
}

fn create_region(kvm: &Kvm, base: u64, size: usize, slot: u32) -> Result<MemoryRegion> {
    let mr = MemoryRegion::new(base, size)
        .map_err(Error::MemoryRegionCreate)?;
    kvm.add_memory_region(slot, base, mr.base_address(), size)
        .map_err(Error::MemoryRegister)?;
    Ok(mr)
}

const BOOT_GDT_OFFSET: usize = 0x500;
const BOOT_IDT_OFFSET: usize = 0x520;

const BOOT_PML4: u64 = 0x9000;
const BOOT_PDPTE: u64 = 0xA000;
const BOOT_PDE: u64 = 0xB000;

pub fn x86_setup_memory(memory: &mut MemoryManager, cmdline: &KernelCmdLine, ncpus: usize, pci_irqs: &[PciIrq]) -> Result<()> {
    load_pm_kernel(memory.guest_ram(), KERNEL_CMDLINE_ADDRESS, cmdline.size())
        .map_err(Error::LoadKernel)?;
    setup_gdt(memory.guest_ram())?;
    setup_boot_pagetables(memory.guest_ram()).map_err(Error::SystemError)?;
    setup_mptable(memory.guest_ram(), ncpus, pci_irqs).map_err(Error::SystemError)?;
    write_cmdline(memory.guest_ram(), cmdline).map_err(Error::SystemError)?;
    Ok(())
}

fn setup_boot_pagetables(memory: &GuestRam) -> system::Result<()> {
    memory.write_int::<u64>(BOOT_PML4, BOOT_PDPTE | 0x3)?;
    memory.write_int::<u64>(BOOT_PDPTE, BOOT_PDE | 0x3)?;
    for i in 0..512_u64 {
        let entry = (i << 21) | 0x83;
        memory.write_int::<u64>(BOOT_PDE + (i * 8), entry)?;
    }
    Ok(())
}

fn write_gdt_table(table: &[u64], memory: &GuestRam) -> system::Result<()> {
    for i in 0..table.len() {
        memory.write_int((BOOT_GDT_OFFSET + i * 8) as u64, table[i])?;
    }
    Ok(())
}

pub fn gdt_entry(flags: u16, base: u32, limit: u32) -> u64 {
    ((((base as u64) & 0xff000000u64) << (56 - 24)) | (((flags as u64) & 0x0000f0ffu64) << 40) |
        (((limit as u64) & 0x000f0000u64) << (48 - 16)) |
        (((base as u64) & 0x00ffffffu64) << 16) | ((limit as u64) & 0x0000ffffu64))
}

pub fn setup_gdt(memory: &GuestRam) -> Result<()> {
    let table = [
        gdt_entry(0,0,0),
        gdt_entry(0xa09b,0,0xfffff),
        gdt_entry(0xc093,0,0xfffff),
        gdt_entry(0x808b,0,0xfffff),
    ];
    write_gdt_table(&table, memory)
        .map_err(Error::SystemError)?;

    memory.write_int::<u64>(BOOT_IDT_OFFSET as u64, 0u64)
        .map_err(Error::SystemError)?;

    Ok(())
}

fn write_cmdline(memory: &GuestRam, cmdline: &KernelCmdLine) -> system::Result<()> {
    let bytes = cmdline.as_bytes();
    let len = bytes.len() as u64;
    memory.write_bytes(KERNEL_CMDLINE_ADDRESS, bytes)?;
    memory.write_int(KERNEL_CMDLINE_ADDRESS + len, 0u8)?;
    Ok(())
}

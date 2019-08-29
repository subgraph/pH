
use std::path::Path;
use std::fs::{File};
use std::io::{self, Read,SeekFrom,Seek};
use byteorder::{LittleEndian,ReadBytesExt};

use crate::memory::{self,GuestRam,KERNEL_ZERO_PAGE};
use crate::vm::{Result,Error,ErrorKind};


// Documentation/x86/boot.txt

const HDR_BOOT_FLAG: u64           = 0x1fe;  // u16
const HDR_HEADER: u64              = 0x202;  // u32
const HDR_TYPE_LOADER: u64         = 0x210;  // u8
const HDR_CMDLINE_PTR: u64         = 0x228;  // u32
const HDR_CMDLINE_SIZE: u64        = 0x238;  // u32
const HDR_KERNEL_ALIGNMENT: u64    = 0x230;  // u32

// Documentation/x86/zero-page.txt

const BOOT_PARAM_E820_ENTRIES: u64 = 0x1e8;
const BOOT_PARAM_E820_MAP: u64     = 0x2d0;

const KERNEL_BOOT_FLAG_MAGIC: u16 = 0xaa55;
const EBDA_START: u64 = 0x0009fc00;
const KERNEL_HDR_MAGIC: u32 = 0x53726448;
const KERNEL_LOADER_OTHER: u8 = 0xff;
const KERNEL_MIN_ALIGNMENT_BYTES: u32 = 0x1000000;

const E820_RAM: u32 = 1;

fn setup_e820(memory: &GuestRam, base: u64) -> Result<()> {
    let ram_size = memory.ram_size() as u64;

    let mut e820_ranges = Vec::new();
    e820_ranges.push((0u64, EBDA_START));

    if ram_size < memory::PCI_MMIO_RESERVED_BASE {
        e820_ranges.push((memory::KVM_KERNEL_LOAD_ADDRESS, ram_size - memory::KVM_KERNEL_LOAD_ADDRESS));
    } else {
        e820_ranges.push((memory::KVM_KERNEL_LOAD_ADDRESS, memory::PCI_MMIO_RESERVED_BASE - memory::KVM_KERNEL_LOAD_ADDRESS));
        e820_ranges.push((memory::HIMEM_BASE, ram_size - memory::HIMEM_BASE));
    }
    memory.write_int::<u8>(base + BOOT_PARAM_E820_ENTRIES, e820_ranges.len() as u8)?;
    for i in 0..e820_ranges.len() {
        let entry_base = base + BOOT_PARAM_E820_MAP + (i as u64 * 20);
        memory.write_int::<u64>(entry_base, e820_ranges[i].0)?;
        memory.write_int::<u64>(entry_base + 8, e820_ranges[i].1)?;
        memory.write_int::<u32>(entry_base + 16, E820_RAM)?;
    }
    Ok(())
}

fn setup_zero_page(memory: &GuestRam, cmdline_addr: u64, cmdline_size: usize) -> Result<()> {
    let base = KERNEL_ZERO_PAGE;
    memory.write_int::<u16>(base + HDR_BOOT_FLAG, KERNEL_BOOT_FLAG_MAGIC)?;
    memory.write_int::<u32>(base + HDR_HEADER, KERNEL_HDR_MAGIC)?;
    memory.write_int::<u8>(base + HDR_TYPE_LOADER, KERNEL_LOADER_OTHER)?;
    memory.write_int::<u32>(base + HDR_CMDLINE_PTR, cmdline_addr as u32)?;
    memory.write_int::<u32>(base + HDR_CMDLINE_SIZE, cmdline_size as u32)?;
    memory.write_int::<u32>(base + HDR_KERNEL_ALIGNMENT, KERNEL_MIN_ALIGNMENT_BYTES)?;

    setup_e820(memory, base)
}

pub fn load_pm_kernel(memory: &GuestRam, path: &Path, cmdline_addr: u64, cmdline_size: usize) -> Result<()> {
    load_elf_kernel(memory, path).map_err(|_| Error::from(ErrorKind::ReadKernelFailed))?;
    setup_zero_page(memory,  cmdline_addr, cmdline_size)
}

pub fn load_elf_kernel(memory: &GuestRam, path: &Path) -> io::Result<()> {
    let mut f = File::open(&path)?;
    f.seek(SeekFrom::Start(32))?;
    let phoff = f.read_u64::<LittleEndian>()?;
    f.seek(SeekFrom::Current(16))?;
    let phnum = f.read_u16::<LittleEndian>()?;
    f.seek(SeekFrom::Start(phoff))?;
    let mut v = Vec::new();
    for _ in 0..phnum {
        let hdr = load_phdr(&f)?;
        if hdr.p_type == 1 {
            v.push(hdr);
        }
    }

    for h in v {
        f.seek(SeekFrom::Start(h.p_offset))?;
        let slice = memory.mut_slice(memory::KVM_KERNEL_LOAD_ADDRESS + h.p_paddr, h.p_filesz as usize).unwrap();
        f.read_exact(slice)?;
    }
    Ok(())
}

fn load_phdr<R: Read+Sized>(mut r: R) -> io::Result<ElfPhdr> {
    let mut phdr: ElfPhdr = Default::default();
    phdr.p_type = r.read_u32::<LittleEndian>()?;
    phdr.p_flags = r.read_u32::<LittleEndian>()?;
    phdr.p_offset = r.read_u64::<LittleEndian>()?;
    phdr.p_vaddr = r.read_u64::<LittleEndian>()?;
    phdr.p_paddr = r.read_u64::<LittleEndian>()?;
    phdr.p_filesz = r.read_u64::<LittleEndian>()?;
    phdr.p_memsz = r.read_u64::<LittleEndian>()?;
    phdr.p_align = r.read_u64::<LittleEndian>()?;
    Ok(phdr)
}

#[derive(Default,Debug)]
struct ElfPhdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}
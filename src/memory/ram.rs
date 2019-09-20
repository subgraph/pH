use std::sync::Arc;
use std::cmp;
use std::mem;

use crate::memory::Mapping;
use crate::memory::mmap::Serializable;
use crate::memory::AddressRange;

use crate::kvm::Kvm;
use crate::vm::{Result,Error,ErrorKind};

pub const HIMEM_BASE: u64 = (1 << 32);
pub const PCI_MMIO_RESERVED_SIZE: usize = (512 << 20);
pub const PCI_MMIO_RESERVED_BASE: u64 = HIMEM_BASE - PCI_MMIO_RESERVED_SIZE as u64;

#[derive(Clone)]
pub struct GuestRam {
    ram_size: usize,
    regions: Arc<Vec<MemoryRegion>>,
}

impl GuestRam {
    pub fn new(ram_size: usize, kvm: &Kvm) -> Result<GuestRam> {
        Ok(GuestRam {
            ram_size,
            regions: Arc::new(create_regions(kvm, ram_size)?),
        })
    }

    pub fn ram_size(&self) -> usize {
        self.ram_size
    }

    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    pub fn write_bytes(&self, guest_address: u64, bytes: &[u8]) -> Result<()> {
        let region = self.find_region(guest_address, bytes.len())?;
        region.write_bytes(guest_address, bytes)
    }

    pub fn read_bytes(&self, guest_address: u64, bytes: &mut [u8]) -> Result<()> {
        let region = self.find_region(guest_address, bytes.len())?;
        region.read_bytes(guest_address, bytes)
    }

    pub fn slice(&self, guest_address: u64, size: usize) -> Result<&[u8]> {
        let region = self.find_region(guest_address, size)?;
        region.slice(guest_address, size)
    }

    pub fn mut_slice(&self, guest_address: u64, size: usize) -> Result<&mut[u8]> {
        let region = self.find_region(guest_address, size)?;
        region.mut_slice(guest_address, size)
    }

    pub fn write_int<T: Serializable>(&self, guest_address: u64, val: T) -> Result<()> {
        let region = self.find_region(guest_address, mem::size_of::<T>())?;
        region.write_int(guest_address, val)
    }

    pub fn read_int<T: Serializable>(&self, guest_address: u64) -> Result<T> {
        let region = self.find_region(guest_address, mem::size_of::<T>())?;
        region.read_int(guest_address)
    }

    #[allow(dead_code)]
    pub fn end_addr(&self) -> u64 {
        self.regions.iter()
            .max_by_key(|r| r.guest_range.end())
            .map_or(0, |r| r.guest_range.end())
    }

    pub fn is_valid_range(&self, guest_address: u64, size: usize) -> bool {
        self.find_region(guest_address, size).is_ok()
    }

    fn find_region(&self, guest_address: u64, size: usize) -> Result<&MemoryRegion> {
        self.regions.iter()
                .find(|r| r.contains(guest_address, size))
                .ok_or_else(|| Error::from(ErrorKind::InvalidAddress(guest_address)))
    }
}

fn add_region(regions: &mut Vec<MemoryRegion>, base: u64, size: usize, kvm: &Kvm) -> Result<()> {
    let slot = regions.len() as u32;
    let mr = MemoryRegion::new(base, size)?;
    kvm.add_memory_region(slot, base, mr.mapping.address(), size)
        .map_err(|e| Error::new(ErrorKind::RegisterMemoryFailed, e))?;
    regions.push(mr);
    Ok(())
}

fn create_regions(kvm: &Kvm, ram_size: usize) -> Result<Vec<MemoryRegion>> {
    let mut regions = Vec::new();

    let lowmem_sz = cmp::min(ram_size, PCI_MMIO_RESERVED_BASE as usize);
    add_region(&mut regions, 0, lowmem_sz, &kvm)?;

    if lowmem_sz < ram_size {
        let himem_sz = ram_size - lowmem_sz;
        add_region(&mut regions, HIMEM_BASE, himem_sz, &kvm)?;
    }

    Ok(regions)
}

struct MemoryRegion {
    guest_range: AddressRange,
    mapping: Mapping,
}

impl MemoryRegion {
    fn new(guest_base: u64, size: usize) -> Result<MemoryRegion> {
        Ok(MemoryRegion{
            guest_range: AddressRange::new(guest_base, size),
            mapping: Mapping::new(size)?,
        })
    }

    fn contains(&self, guest_addr: u64, size: usize) -> bool { self.guest_range.contains(guest_addr, size) }

    fn checked_offset(&self, guest_addr: u64, size: usize) -> Result<usize> {
        if self.contains(guest_addr, size) {
            Ok(self.guest_range.offset_of(guest_addr))
        } else {
            Err(Error::from(ErrorKind::InvalidAddress(guest_addr)))
        }
    }

    pub fn write_bytes(&self, guest_address: u64, bytes: &[u8]) -> Result<()> {
        let offset = self.checked_offset(guest_address, bytes.len())?;
        self.mapping.write_bytes(offset, bytes)
    }

    pub fn read_bytes(&self, guest_address: u64, bytes: &mut [u8]) -> Result<()> {
        let offset = self.checked_offset(guest_address, bytes.len())?;
        self.mapping.read_bytes(offset, bytes)
    }

    pub fn slice(&self, guest_address: u64, size: usize) -> Result<&[u8]> {
        let offset = self.checked_offset(guest_address, size)?;
        self.mapping.slice(offset, size)
    }

    pub fn mut_slice(&self, guest_address: u64, size: usize) -> Result<&mut [u8]> {
        let offset = self.checked_offset(guest_address, size)?;
        self.mapping.mut_slice(offset, size)
    }

    pub fn write_int<T: Serializable>(&self, guest_address: u64, val: T) -> Result<()> {
        let offset = self.checked_offset(guest_address, mem::size_of::<T>())?;
        self.mapping.write_int(offset, val)
    }

    pub fn read_int<T: Serializable>(&self, guest_address: u64) -> Result<T> {
        let offset = self.checked_offset(guest_address, mem::size_of::<T>())?;
        self.mapping.read_int(offset)
    }
}

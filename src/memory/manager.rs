use std::collections::HashMap;
use std::os::unix::io::{AsRawFd,RawFd};
use std::sync::{Arc, RwLock};

use crate::memory::{GuestRam, SystemAllocator, Mapping, Error, Result};
use crate::kvm::Kvm;
use crate::system::{BitVec, FileDesc};
use crate::memory::drm::{DrmBufferAllocator, DrmDescriptor};
use std::io::SeekFrom;

#[derive(Clone)]
pub struct MemoryManager {
    kvm: Kvm,
    ram: GuestRam,
    device_memory: Arc<RwLock<DeviceMemory>>,
    drm_allocator: Option<DrmBufferAllocator>,
}

impl MemoryManager {

    pub fn new(kvm: Kvm, ram: GuestRam, allocator: SystemAllocator, use_drm: bool) -> Result<Self> {
        let device_memory = RwLock::new(DeviceMemory::new(ram.region_count(), allocator)).into();
        let drm_allocator = if use_drm {
            DrmBufferAllocator::open().ok()
        } else {
            None
        };
        Ok(MemoryManager {
            kvm, ram, device_memory,
            drm_allocator,
        })
    }

    pub fn guest_ram(&self) -> &GuestRam {
        &self.ram
    }

    pub fn kvm_mut(&mut self) -> &mut Kvm {
        &mut self.kvm
    }

    pub fn kvm(&self) -> &Kvm {
        &self.kvm
    }

    pub fn register_device_memory(&self, fd: RawFd, size: usize) -> Result<(u64, u32)> {
        let mut devmem = self.device_memory.write().unwrap();
        devmem.register(self.kvm(), fd, size)
    }

    pub fn unregister_device_memory(&self, slot: u32) -> Result<()> {
        let mut devmem = self.device_memory.write().unwrap();
        devmem.unregister(self.kvm(), slot)
    }

    pub fn drm_available(&self) -> bool {
        self.drm_allocator.is_some()
    }

    pub fn allocate_drm_buffer(&self, width: u32, height: u32, format: u32) -> Result<(u64, u32, FileDesc, DrmDescriptor)> {
        if let Some(drm_allocator) = self.drm_allocator.as_ref() {
            let (fd, desc) = drm_allocator.allocate(width, height, format)?;
            let size = fd.seek(SeekFrom::End(0)).map_err(Error::CreateBuffer)?;

            let (pfn, slot) = self.register_device_memory(fd.as_raw_fd(), size as usize)?;
            Ok((pfn, slot, fd, desc))
        } else {
            Err(Error::NoDrmAllocator)
        }
    }
}

pub struct MemoryRegistration {
    guest_addr: u64,
    _mapping: Mapping,
}

impl MemoryRegistration {
    fn new(guest_addr: u64, mapping: Mapping)-> Self {
        MemoryRegistration { guest_addr, _mapping: mapping }
    }
}

struct DeviceMemory {
    slots: BitVec,
    mappings: HashMap<u32, MemoryRegistration>,
    allocator: SystemAllocator,
}

impl DeviceMemory {
    fn new(ram_region_count: usize, allocator: SystemAllocator) -> DeviceMemory {
        let mut slots = BitVec::new();
        for i in 0..ram_region_count {
            slots.set_bit(i);
        }
        DeviceMemory {
            slots, mappings: HashMap::new(), allocator
        }
    }

    fn register(&mut self, kvm: &Kvm, fd: RawFd, size: usize) -> Result<(u64, u32)> {
        let mapping = Mapping::new_from_fd(fd, size)
            .map_err(Error::MappingFailed)?;

        let (addr, slot) = self.allocate_addr_and_slot(size)?;

        if let Err(e) = kvm.add_memory_region(slot, addr, mapping.address(), size) {
            self.free_addr_and_slot(addr, slot);
            Err(Error::RegisterMemoryFailed(e))
        } else {
            self.mappings.insert(slot, MemoryRegistration::new(addr, mapping));
            Ok((addr >> 12, slot))
        }
    }

    fn unregister(&mut self, kvm: &Kvm, slot: u32) -> Result<()> {
        if let Some(registration) = self.mappings.remove(&slot) {
            kvm.remove_memory_region(slot)
                .map_err(Error::UnregisterMemoryFailed)?;
            self.free_addr_and_slot(registration.guest_addr, slot);
        }
        Ok(())
    }

    fn allocate_addr_and_slot(&mut self, size: usize) -> Result<(u64, u32)> {
        let addr = self.allocator.allocate_device_memory(size)
            .ok_or(Error::DeviceMemoryAllocFailed)?;
        Ok((addr, self.allocate_slot()))
    }

    fn free_addr_and_slot(&mut self, addr: u64, slot: u32) {
        self.allocator.free_device_memory(addr);
        self.free_slot(slot);
    }

    fn allocate_slot(&mut self) -> u32 {
        let slot = self.slots.first_unset();
        self.slots.set_bit(slot);
        slot as u32
    }

    fn free_slot(&mut self, slot: u32) {
        self.slots.clear_bit(slot as usize)
    }
}

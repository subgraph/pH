use crate::memory::AddressRange;
use std::collections::BTreeMap;
use std::sync::{Mutex, Arc};

#[derive(Clone)]
pub struct SystemAllocator {
    device_memory: AddressAllocator,
}

impl SystemAllocator {
    pub fn new(device_range: AddressRange) -> Self {
        let device_memory = AddressAllocator::new(device_range, 4096);
        SystemAllocator { device_memory }
    }

    pub fn free_device_memory(&self, base: u64) -> bool {
        self.device_memory.free(base)
    }

    pub fn allocate_device_memory(&self, size: usize) -> Option<u64> {
        self.device_memory.allocate(size)
    }
}

#[derive(Clone)]
struct AddressAllocator {
    range: AddressRange,
    default_alignment: usize,
    allocations: Arc<Mutex<BTreeMap<u64, AddressRange>>>,
}

impl AddressAllocator {
    fn new(range: AddressRange, default_alignment: usize) -> Self {
        let allocations = Arc::new(Mutex::new(BTreeMap::new()));
        AddressAllocator { range, default_alignment, allocations }
    }

    fn align_addr_to(addr: u64, alignment: u64) -> u64 {
        let adjust = if addr % alignment != 0 {
            alignment - (addr % alignment)
        } else {
            0
        };
        addr + adjust
    }

    fn allocate(&self, size: usize) -> Option<u64> {
        self.allocate_aligned(size, self.default_alignment)
    }

    fn allocate_aligned(&self, size: usize, alignment: usize) -> Option<u64> {
        self.first_available(size, alignment)
    }

    fn free(&self, base: u64) -> bool {
        let mut map = self.allocations.lock().unwrap();
        map.remove(&base).is_some()
    }

    fn first_available(&self, size: usize, alignment: usize) -> Option<u64> {
        if size == 0 {
            return None;
        }

        let mut map = self.allocations.lock().unwrap();
        let base = self.first_available_base_addr(&map, size, alignment);
        let size = size as usize;
        if self.range.contains(base, size) {
            map.insert(base, AddressRange::new(base, size));
            return Some(base);
        }
        None
    }

    // Return lowest base address of requested alignment which does not
    // conflict with any currently allocated range
    fn first_available_base_addr(&self, map: &BTreeMap<u64, AddressRange>, size: usize, alignment: usize) -> u64 {
        let mut base = Self::align_addr_to(self.range.base(), alignment as u64);
        for alloc in map.values() {
            // Alignment adjustment may have placed address beyond start of next
            // allocation.
            if let Some(gap_size) = alloc.base().checked_sub(base) {
                if (gap_size as usize) >= size {
                    return base;
                }
            }
            if base < alloc.end() {
                base = Self::align_addr_to(alloc.end(), alignment as u64);
            }
        }
        base
    }
}

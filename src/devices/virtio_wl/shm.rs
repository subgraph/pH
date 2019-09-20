use std::os::unix::io::{AsRawFd,RawFd};

use crate::memory::MemoryManager;
use crate::system::MemoryFd;

use crate::devices::virtio_wl::{
    consts::{VIRTIO_WL_VFD_MAP, VIRTIO_WL_VFD_WRITE},
    Error, Result, VfdObject
};

pub struct VfdSharedMemory {
    vfd_id: u32,
    flags: u32,
    mm: MemoryManager,
    memfd: Option<MemoryFd>,
    slot: u32,
    pfn: u64,
}

impl VfdSharedMemory {
    fn round_to_page_size(n: usize) -> usize {
        let mask = 4096 - 1;
        (n + mask) & !mask
    }

    pub fn new(vfd_id: u32, transition_flags: bool, mm: MemoryManager, memfd: MemoryFd, slot: u32, pfn: u64) -> Self {
        let flags = if transition_flags { 0 } else { VIRTIO_WL_VFD_WRITE | VIRTIO_WL_VFD_MAP};
        let memfd = Some(memfd);
        VfdSharedMemory { vfd_id, flags, mm, memfd, slot, pfn }
    }

    pub fn create(vfd_id: u32, transition_flags: bool, size: u32, mm: &MemoryManager) -> Result<Self> {
        let size = Self::round_to_page_size(size as usize);
        let memfd = MemoryFd::new_memfd(size, true)
            .map_err(Error::ShmAllocFailed)?;
        let (pfn, slot) = mm.register_device_memory(memfd.as_raw_fd(), size)
            .map_err(Error::RegisterMemoryFailed)?;
        Ok(Self::new(vfd_id, transition_flags, mm.clone(), memfd, slot, pfn))
    }
}

impl VfdObject for VfdSharedMemory {
    fn id(&self) -> u32 {
        self.vfd_id
    }

    fn send_fd(&self) -> Option<RawFd> {
        self.memfd.as_ref().map(AsRawFd::as_raw_fd)
    }

    fn flags(&self) -> u32 {
        self.flags
    }

    fn pfn_and_size(&self) -> Option<(u64, u64)> {
        if let Some(memfd) = self.memfd.as_ref() {
            Some((self.pfn, memfd.size() as u64))
        } else {
            None
        }
    }

    fn close(&mut self) -> Result<()> {
        if let Some(_) = self.memfd.take() {
            self.mm.unregister_device_memory(self.slot)
                .map_err(Error::RegisterMemoryFailed)?;
        }
        Ok(())
    }
}

use crate::system::{MemoryFd, BitVec};
use crate::disk::{Result, Error, SECTOR_SIZE, DiskImage};
use std::io::SeekFrom;

pub struct MemoryOverlay {
    memory: MemoryFd,
    written_sectors: BitVec,
}

impl MemoryOverlay {
    pub fn new() -> Result<Self> {
        let memory = MemoryFd::new_memfd(0, false)
            .map_err(Error::MemoryOverlayCreate)?;
        let written_sectors = BitVec::new();
        Ok(MemoryOverlay { memory, written_sectors })
    }

    pub fn write_sectors(&mut self, start: u64, buffer: &[u8]) -> Result<()> {
        let sector_count = buffer.len() / SECTOR_SIZE;
        let len = sector_count * SECTOR_SIZE;
        let seek_offset = SeekFrom::Start(start * SECTOR_SIZE as u64);

        self.memory.fd_mut()
            .seek(seek_offset)
            .map_err(Error::DiskSeek)?;

        self.memory.fd_mut()
            .write_all(&buffer[..len])
            .map_err(Error::DiskWrite)?;

        for n in 0..sector_count {
            let idx = start as usize + n;
            self.written_sectors.set_bit(idx);
        }
        Ok(())
    }

    pub fn read_sectors<D: DiskImage>(&mut self, disk: &mut D, start: u64, buffer: &mut [u8]) -> Result<()> {
        let sector_count = buffer.len() / SECTOR_SIZE;
        if (0..sector_count).all(|i| !self.written_sectors.get_bit(i)) {
            return disk.read_sectors(start, buffer);
        }

        for n in 0..sector_count {
            let sector = start + n as u64;
            let offset = n * SECTOR_SIZE;
            let sector_buffer = &mut buffer[offset..offset+SECTOR_SIZE];
            if self.written_sectors.get_bit(sector as usize) {
                self.read_single_sector(sector, sector_buffer)?;
            } else {
                disk.read_sectors(sector, sector_buffer)?;
            }
        }
        Ok(())
    }

    fn read_single_sector(&mut self, sector: u64, buffer: &mut [u8]) -> Result<()> {
        assert_eq!(buffer.len(), SECTOR_SIZE);
        let offset = SeekFrom::Start(sector * SECTOR_SIZE as u64);
        self.memory.fd_mut().seek(offset)
            .map_err(Error::DiskSeek)?;
        self.memory.fd_mut().read_exact(buffer)
            .map_err(Error::DiskRead)?;
        Ok(())
    }

}
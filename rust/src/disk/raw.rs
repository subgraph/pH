use crate::disk::{Result, Error, DiskImage, SECTOR_SIZE, generate_disk_image_id, OpenType};
use std::fs::{File, OpenOptions};
use std::io::{Write, Read, SeekFrom, Seek};
use crate::disk::Error::DiskRead;
use crate::disk::memory::MemoryOverlay;
use std::path::Path;

pub struct RawDiskImage {
    file: File,
    offset: usize,
    nsectors: u64,
    read_only: bool,
    disk_image_id: Vec<u8>,
    overlay: Option<MemoryOverlay>,
}

impl RawDiskImage {
    #[allow(dead_code)]
    pub fn open<P: AsRef<Path>>(path: P, open_type: OpenType) -> Result<Self> {
        Self::open_with_offset(path, open_type, 0)
    }

    pub fn open_with_offset<P: AsRef<Path>>(path: P, open_type: OpenType, offset: usize) -> Result<Self> {
        let path = path.as_ref();
        let meta = path.metadata()
            .map_err(|e| Error::DiskOpen(path.into(), e))?;

        if meta.len() < offset as u64 {
            return Err(Error::DiskOpenTooShort(path.into()))
        }

        let nsectors = (meta.len() - offset as u64) / SECTOR_SIZE as u64;

        let file = OpenOptions::new()
            .read(true)
            .write(open_type == OpenType::ReadWrite)
            .open(path)
            .map_err(|e| Error::DiskOpen(path.into(), e))?;


        let disk = match open_type {
            OpenType::MemoryOverlay => {
                let overlay = MemoryOverlay::new()?;
                Self::new(file, nsectors, offset, false, Some(overlay))
            }
            OpenType::ReadOnly => {
                Self::new(file, nsectors, offset, true, None)
            }
            OpenType::ReadWrite => {
                Self::new(file, nsectors, offset, false, None)
            }
        };
        Ok(disk)
    }

    pub fn new(file: File, nsectors: u64, offset: usize, read_only: bool, overlay: Option<MemoryOverlay>) -> Self {
        let disk_image_id = generate_disk_image_id(&file);
        RawDiskImage { file, nsectors, read_only, offset, disk_image_id, overlay }
    }
}

impl DiskImage for RawDiskImage {
    fn read_only(&self) -> bool {
        self.read_only
    }

    fn sector_count(&self) -> u64 {
        self.nsectors
    }

    fn disk_file(&self) -> &File {
        &self.file
    }

    fn seek_to_sector(&self, sector: u64) -> Result<()> {
        if sector > self.sector_count() {
            return Err(Error::BadSectorOffset(sector));
        }
        let offset = SeekFrom::Start(sector * SECTOR_SIZE as u64 + self.offset as u64);
        self.disk_file().seek(offset)
            .map_err(Error::DiskSeek)?;
        Ok(())
    }

    fn write_sectors(&mut self, start_sector: u64, buffer: &[u8]) -> Result<()> {
        if let Some(ref mut overlay) = self.overlay {
            return overlay.write_sectors(start_sector, buffer);
        }
        if self.read_only {
            return Err(Error::ReadOnly)
        }
        self.seek_to_sector(start_sector)?;
        let len = (buffer.len() / SECTOR_SIZE) * SECTOR_SIZE;
        self.file.write_all(&buffer[..len])
            .map_err(Error::DiskWrite)?;
        Ok(())
    }

    fn read_sectors(&mut self, start_sector: u64, buffer: &mut [u8]) -> Result<()> {
        if let Some(mut overlay) = self.overlay.take() {
            let ret = overlay.read_sectors(self, start_sector, buffer);
            self.overlay.replace(overlay);
            return ret;
        }

        self.seek_to_sector(start_sector)?;
        let len = (buffer.len() / SECTOR_SIZE) * SECTOR_SIZE;
        self.file.read_exact(&mut buffer[..len])
            .map_err(DiskRead)?;
        Ok(())
    }

    fn disk_image_id(&self) -> &[u8] {
        &self.disk_image_id
    }
}
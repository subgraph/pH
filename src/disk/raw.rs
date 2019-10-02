use crate::disk::{Result, Error, DiskImage, SECTOR_SIZE, generate_disk_image_id, OpenType};
use std::fs::{File, OpenOptions};
use std::io::{Write, Read, SeekFrom, Seek};
use crate::disk::Error::DiskRead;
use crate::disk::memory::MemoryOverlay;
use std::path::{PathBuf, Path};


pub struct RawDiskImage {
    path: PathBuf,
    open_type: OpenType,
    file: Option<File>,
    offset: usize,
    nsectors: u64,
    disk_image_id: Vec<u8>,
    overlay: Option<MemoryOverlay>,
}

impl RawDiskImage {
    fn get_nsectors(path: &Path, offset: usize) -> Result<u64> {
        if let Ok(meta) = path.metadata() {
            Ok((meta.len() - offset as u64) / SECTOR_SIZE as u64)
        } else {
            Err(Error::ImageDoesntExit(path.to_path_buf()))
        }
    }

    #[allow(dead_code)]
    pub fn new<P: Into<PathBuf>>(path: P, open_type: OpenType) -> Result<Self> {
        Self::new_with_offset(path, open_type, 0)
    }

    pub fn new_with_offset<P: Into<PathBuf>>(path: P, open_type: OpenType, offset: usize) -> Result<Self> {
        let path = path.into();
        let nsectors = Self::get_nsectors(&path, offset)?;
        Ok(RawDiskImage {
            path,
            open_type,
            file: None,
            offset,
            nsectors,
            disk_image_id: Vec::new(),
            overlay: None,
        })
    }

}

impl DiskImage for RawDiskImage {
    fn open(&mut self) -> Result<()> {
        let meta = self.path.metadata()
            .map_err(|e| Error::DiskOpen(self.path.clone(), e))?;

        if meta.len() < self.offset as u64 {
            return Err(Error::DiskOpenTooShort(self.path.clone()))
        }

        let file = OpenOptions::new()
            .read(true)
            .write(self.open_type == OpenType::ReadWrite)
            .open(&self.path)
            .map_err(|e| Error::DiskOpen(self.path.clone(), e))?;

        self.disk_image_id = generate_disk_image_id(&file);
        self.file = Some(file);

        if self.open_type == OpenType::MemoryOverlay {
            let overlay = MemoryOverlay::new()?;
            self.overlay = Some(overlay);
        }
        Ok(())
    }

    fn read_only(&self) -> bool {
        self.open_type == OpenType::ReadOnly
    }

    fn sector_count(&self) -> u64 {
        self.nsectors
    }

    fn disk_file(&mut self) -> Result<&mut File> {
        self.file.as_mut().ok_or(Error::NotOpen)
    }

    fn seek_to_sector(&mut self, sector: u64) -> Result<()> {
        if sector > self.sector_count() {
            return Err(Error::BadSectorOffset(sector));
        }
        let offset = SeekFrom::Start(sector * SECTOR_SIZE as u64 + self.offset as u64);
        let disk = self.disk_file()?;
        disk.seek(offset)
            .map_err(Error::DiskSeek)?;
        Ok(())
    }

    fn write_sectors(&mut self, start_sector: u64, buffer: &[u8]) -> Result<()> {
        if let Some(ref mut overlay) = self.overlay {
            return overlay.write_sectors(start_sector, buffer);
        }
        if self.read_only() {
            return Err(Error::ReadOnly)
        }
        self.seek_to_sector(start_sector)?;
        let len = (buffer.len() / SECTOR_SIZE) * SECTOR_SIZE;
        let file = self.disk_file()?;
        file.write_all(&buffer[..len])
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
        let file = self.disk_file()?;
        file.read_exact(&mut buffer[..len])
            .map_err(DiskRead)?;
        Ok(())
    }

    fn disk_image_id(&self) -> &[u8] {
        &self.disk_image_id
    }
}
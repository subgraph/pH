use crate::disk::{Result, DiskImage, SECTOR_SIZE, RawDiskImage, OpenType};
use std::fs::File;
use std::path::Path;

// skip 4096 byte realmfs header
const HEADER_SECTOR_COUNT: usize = 8;

pub struct RealmFSImage {
    raw: RawDiskImage,
}

// Just pass everything through to raw image for now
impl RealmFSImage {
    pub fn open<P: AsRef<Path>>(path: P, read_only: bool) -> Result<Self> {
        let open_type = if read_only { OpenType::ReadOnly } else { OpenType::MemoryOverlay };
        let raw = RawDiskImage::open_with_offset(path, open_type, HEADER_SECTOR_COUNT * SECTOR_SIZE)?;
        Ok(RealmFSImage { raw })
    }
}

impl DiskImage for RealmFSImage {
    fn read_only(&self) -> bool {
        self.raw.read_only()
    }

    fn sector_count(&self) -> u64 {
        self.raw.sector_count()
    }

    fn disk_file(&self) -> &File {
        self.raw.disk_file()
    }

    fn write_sectors(&mut self, start_sector: u64, buffer: &[u8]) -> Result<()> {
        self.raw.write_sectors(start_sector, buffer)
    }

    fn read_sectors(&mut self, start_sector: u64, buffer: &mut [u8]) -> Result<()> {
        self.raw.read_sectors(start_sector, buffer)
    }

    fn disk_image_id(&self) -> &[u8] {
        self.raw.disk_image_id()
    }
}

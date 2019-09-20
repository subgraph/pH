use crate::disk::{Result, DiskImage, SECTOR_SIZE, RawDiskImage, OpenType};
use std::fs::File;
use std::path::PathBuf;

// skip 4096 byte realmfs header
const HEADER_SECTOR_COUNT: usize = 8;

pub struct RealmFSImage {
    raw: RawDiskImage,
}

// Just pass everything through to raw image for now
impl RealmFSImage {
    pub fn new<P: Into<PathBuf>>(path: P, open_type: OpenType) -> Self {
        let offset = HEADER_SECTOR_COUNT * SECTOR_SIZE;
        let raw = RawDiskImage::new_with_offset(path, open_type, offset);
        RealmFSImage { raw }
    }
    pub fn open(&mut self) -> Result<()> {
        self.raw.open()
    }
}

impl DiskImage for RealmFSImage {
    fn read_only(&self) -> bool {
        self.raw.read_only()
    }

    fn sector_count(&self) -> u64 {
        self.raw.sector_count()
    }

    fn disk_file(&mut self) -> Result<&mut File> {
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

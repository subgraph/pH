use std::{io, error, fmt, result, cmp};
use std::fs::File;
use std::os::linux::fs::MetadataExt;
use std::io::{SeekFrom, Seek};

use crate::system;

mod realmfs;
mod raw;
mod memory;

pub use raw::RawDiskImage;
pub use realmfs::RealmFSImage;
use std::path::PathBuf;

const SECTOR_SIZE: usize = 512;

#[derive(Debug,PartialEq)]
pub enum OpenType {
    ReadOnly,
    ReadWrite,
    MemoryOverlay,
}

pub trait DiskImage: Sync+Send {
    fn read_only(&self) -> bool;
    fn sector_count(&self) -> u64;
    fn disk_file(&mut self) -> Result<&mut File>;

    fn seek_to_sector(&mut self, sector: u64) -> Result<()> {
        if sector > self.sector_count() {
            return Err(Error::BadSectorOffset(sector));
        }
        let offset = SeekFrom::Start(sector * SECTOR_SIZE as u64);
        let file = self.disk_file()?;
        file.seek(offset)
            .map_err(Error::DiskSeek)?;
        Ok(())
    }
    fn write_sectors(&mut self, start_sector: u64, buffer: &[u8]) -> Result<()>;
    fn read_sectors(&mut self, start_sector: u64, buffer: &mut [u8]) -> Result<()>;
    fn flush(&mut self) -> Result<()> { Ok(()) }

    fn disk_image_id(&self) -> &[u8];
}

fn generate_disk_image_id(disk_file: &File) -> Vec<u8> {
    const VIRTIO_BLK_ID_BYTES: usize = 20;
    let meta = match disk_file.metadata() {
        Ok(meta) => meta,
        Err(_) => return vec![0u8; VIRTIO_BLK_ID_BYTES]
    };
    let dev_id = format!("{}{}{}", meta.st_dev(), meta.st_rdev(), meta.st_ino());
    let bytes = dev_id.as_bytes();
    let len = cmp::min(bytes.len(), VIRTIO_BLK_ID_BYTES);
    Vec::from(&bytes[..len])
}

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    ReadOnly,
    DiskOpen(PathBuf,io::Error),
    DiskOpenTooShort(PathBuf),
    DiskRead(io::Error),
    DiskWrite(io::Error),
    DiskSeek(io::Error),
    BadSectorOffset(u64),
    MemoryOverlayCreate(system::Error),
    NotOpen,
}

impl error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Error::*;
        match self {
            ReadOnly => write!(f, "attempted write to read-only device"),
            DiskOpen(path, err) => write!(f, "failed to open disk image {}: {}", path.display(), err),
            DiskOpenTooShort(path) => write!(f, "failed to open disk image {} because file is too short", path.display()),
            DiskRead(err) => write!(f, "error reading from disk image: {}", err),
            DiskWrite(err) => write!(f, "error writing to disk image: {}", err),
            DiskSeek(err) => write!(f, "error seeking to offset on disk image: {}", err),
            BadSectorOffset(sector) => write!(f, "attempt to access invalid sector offset {}", sector),
            MemoryOverlayCreate(err) => write!(f, "failed to create memory overlay: {}", err),
            NotOpen => write!(f, "disk not open"),
        }
    }
}
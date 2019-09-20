use std::{fs, io};

use crate::devices::virtio_9p::{
    pdu::PduParser, file::Qid,
};

pub struct Directory {
    entries: Vec<P9DirEntry>,
}

impl Directory {

    pub fn new() -> Directory {
        Directory { entries: Vec::new() }
    }

    pub fn write_entries(&self, pp: &mut PduParser, offset: u64, size: usize) -> io::Result<()> {
        let mut remaining = size;

        pp.w32(0)?;
        for entry in self.entries.iter()
            .skip_while(|e| e.offset <= offset)
        {
            if entry.size() > remaining {
                break;
            }
            entry.write(pp)?;
            remaining -= entry.size();
        }
        pp.w32_at(0, (size - remaining) as u32);
        Ok(())
    }

    pub fn push_entry(&mut self, entry: P9DirEntry) {
        self.entries.push(entry)
    }
}

pub struct P9DirEntry{
    qid: Qid,
    offset: u64,
    dtype: u8,
    name: String,
}

impl P9DirEntry {
    pub fn new(qid: Qid, offset: u64, dtype: u8, name: &str) -> Self {
        let name = name.to_string();
        let offset = offset + Self::size_with_name(&name) as u64;

        P9DirEntry { qid, offset, dtype, name }
    }
    pub fn from_direntry(entry: fs::DirEntry, offset: u64) -> io::Result<Self> {
        let meta = entry.metadata()?;
        let qid = Qid::from_metadata(&meta);
        let dtype = if meta.is_dir() {
            libc::DT_DIR
        } else if meta.is_file() {
            libc::DT_REG
        } else {
            libc::DT_UNKNOWN
        };
        let name = match entry.file_name().into_string() {
            Ok(s) => s,
            _ => return Err(io::Error::from_raw_os_error(libc::EINVAL)),
        };
        // qid + offset + dtype + strlen + name
        let offset = offset + Self::size_with_name(&name) as u64;
        Ok(P9DirEntry{
            qid, offset,
            dtype, name,
        })
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    fn size(&self) -> usize {
        Self::size_with_name(&self.name)
    }

    fn size_with_name(name: &str) -> usize {
        // qid + offset + dtype + strlen + name
        13 + 8 + 1 + 2 + name.as_bytes().len()
    }

    fn write(&self, pp: &mut PduParser) -> io::Result<()> {
        self.qid.write(pp)?;
        pp.w64(self.offset)?;
        pp.w8(self.dtype)?;
        pp.write_string(&self.name)?;
        Ok(())
    }
}

use std::fs::Metadata;
const P9_RLERROR: u8 = 7;
use byteorder::{LittleEndian,ReadBytesExt,WriteBytesExt};
use std::io::{self,Read,Write};
use std::os::linux::fs::MetadataExt;
use std::os::unix::ffi::OsStrExt;
use std::ffi::OsStr;
use crate::memory::GuestRam;
use crate::virtio::Chain;

use super::filesystem::StatFs;

use libc;

const P9_STATS_BASIC: u64 =  0x000007ff;

const P9_HEADER_LEN: usize = 7;

const P9_QTFILE: u8 = 0x00;
const P9_QTLINK: u8 = 0x01;
const _P9_QTSYMLINK: u8 = 0x02;

const P9_QTDIR: u8 = 0x80;

pub struct PduParser<'a> {
    memory: GuestRam,
    pub chain: &'a mut Chain,

    size: u32,
    cmd: u8,
    tag: u16,
    reply_start_addr: u64,
}

#[derive(Default)]
pub struct P9Attr {
    valid: u32,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    atime_sec: u64,
    atime_nsec: u64,
    mtime_sec: u64,
    mtime_nsec: u64,
}

impl P9Attr {
    const MODE: u32 = (1 << 0);
    const UID: u32 = (1 << 1);
    const GID: u32 = (1 << 2);
    const SIZE: u32 = (1 << 3);
    const ATIME: u32 = (1 << 4);
    const MTIME: u32 = (1 << 5);
    const CTIME: u32 = (1 << 6);
    const ATIME_SET: u32 = (1 << 7);
    const MTIME_SET: u32 = (1 << 8);
    const MASK: u32 = 127;
    const NO_UID: u32 = 0xFFFFFFFF;

    fn new() -> P9Attr {
        P9Attr { ..Default::default() }
    }

    fn is_valid(&self, flag: u32) -> bool {
        self.valid & flag != 0
    }

    pub fn has_mode(&self) -> bool { self.is_valid(P9Attr::MODE) }
    pub fn has_atime(&self) -> bool { self.is_valid(P9Attr::ATIME) }
    pub fn has_atime_set(&self) -> bool { self.is_valid(P9Attr::ATIME_SET) }
    pub fn has_mtime(&self) -> bool { self.is_valid(P9Attr::MTIME) }
    pub fn has_mtime_set(&self) -> bool { self.is_valid(P9Attr::MTIME_SET) }
    pub fn has_chown(&self) -> bool {
        self.valid & P9Attr::MASK == P9Attr::CTIME||
                self.is_valid(P9Attr::UID|P9Attr::GID)
    }
    pub fn has_size(&self) -> bool { self.is_valid(P9Attr::SIZE) }

    pub fn mode(&self) -> u32 {
        self.mode
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn chown_ids(&self) -> (u32, u32) {
        let uid = if self.is_valid(P9Attr::UID)
            { self.uid } else { P9Attr::NO_UID };
        let gid = if self.is_valid(P9Attr::GID)
            { self.gid } else { P9Attr::NO_UID };
        (uid, gid)
    }

    pub fn atime(&self) -> (u64, u64) {
        (self.atime_sec, self.atime_nsec)
    }

    pub fn mtime(&self) -> (u64, u64) {
            (self.mtime_sec, self.mtime_nsec)
    }

    fn parse(&mut self, pp: &mut PduParser) -> io::Result<()> {
        self.valid = pp.r32()?;
        self.mode = pp.r32()?;
        self.uid = pp.r32()?;
        self.gid = pp.r32()?;
        self.size = pp.r64()?;
        self.atime_sec = pp.r64()?;
        self.atime_nsec = pp.r64()?;
        self.mtime_sec = pp.r64()?;
        self.mtime_nsec = pp.r64()?;
        Ok(())
    }
}


impl <'a> PduParser<'a> {
    pub fn new(chain: &'a mut Chain, memory: GuestRam) -> PduParser<'a> {
        PduParser{ memory, chain, size: 0, cmd: 0, tag: 0, reply_start_addr: 0 }
    }

    pub fn command(&mut self) -> io::Result<u8> {
        self.size = self.r32()?;
        self.cmd = self.r8()?;
        self.tag = self.r16()?;
        Ok(self.cmd)
    }

    pub fn read_done(&mut self) -> io::Result<()> {
        // XXX unwrap
        self.reply_start_addr = self.chain.current_write_address(8).unwrap();
        // reserve header
        self.w32(0)?;
        self.w8(0)?;
        self.w16(0)?;
        Ok(())
    }

    pub fn bail_err(&mut self, error: io::Error) -> io::Result<()> {
        if self.reply_start_addr == 0 {
            self.read_done()?;
        }

        let err = match error.raw_os_error() {
            Some(errno) => errno as u32,
            None => 0,
        };

        self._w32_at(0,P9_HEADER_LEN as u32 + 4);
        self._w8_at(4, P9_RLERROR);
        self._w16_at(5, self.tag);
        self._w32_at(7, err);
        self.chain.flush_chain();
        Ok(())
    }

    #[allow(dead_code)]
    pub fn w8_at(&self, offset: usize, val: u8) {
        self._w8_at(offset + P9_HEADER_LEN, val);
    }

    pub fn _w8_at(&self, offset: usize, val: u8) {
        self.memory.write_int::<u8>(self.reply_start_addr + offset as u64,  val).unwrap();
    }

    #[allow(dead_code)]
    pub fn w16_at(&self, offset: usize, val: u16) {
        self._w16_at(offset + P9_HEADER_LEN, val);
    }

    pub fn _w16_at(&self, offset: usize, val: u16) {
        self.memory.write_int::<u16>(self.reply_start_addr + offset as u64,  val).unwrap();
    }

    pub fn w32_at(&self, offset: usize, val: u32) {
        self._w32_at(offset + P9_HEADER_LEN, val);
    }

    pub fn _w32_at(&self, offset: usize, val: u32) {
        self.memory.write_int::<u32>(self.reply_start_addr + offset as u64,  val).unwrap();
    }


    pub fn write_done(&mut self) -> io::Result<()> {
        self._w32_at(0, self.chain.get_wlen() as u32);
        let cmd = self.cmd + 1;
        self._w8_at(4, cmd);
        let tag = self.tag;
        self._w16_at(5, tag);
        self.chain.flush_chain();
        Ok(())
    }

    pub fn read_string(&mut self) -> io::Result<String> {
        let len = self.r16()?;
        if len == 0 {
            return Ok(String::new());
        }
        let mut buf = vec![0u8; len as usize];
        self.chain.read_exact(&mut buf)?;
        let s = String::from_utf8(buf)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "bad 9p string"))?;
        Ok(s)
    }

    pub fn read_attr(&mut self) -> io::Result<P9Attr> {
        let mut attr = P9Attr::new();
        attr.parse(self)?;
        Ok(attr)
    }

    pub fn write_string(&mut self, str: &str) -> io::Result<()> {
        self.w16(str.len() as u16)?;
        self.chain.write_all(str.as_bytes())
    }

    pub fn write_os_string(&mut self, str: &OsStr) -> io::Result<()> {
        self.w16(str.len() as u16)?;
        self.chain.write_all(str.as_bytes())
    }


    fn is_lnk(meta: &Metadata) -> bool {
        meta.st_mode() & libc::S_IFMT == libc::S_IFLNK
    }

    fn meta_to_qtype(meta: &Metadata) -> u8 {
        if meta.is_dir() {
            P9_QTDIR
        } else if PduParser::is_lnk(meta) {
            P9_QTLINK
        } else {
            P9_QTFILE
        }
    }

    pub fn write_qid(&mut self, meta: &Metadata) -> io::Result<()> {
        // type
        self.w8(PduParser::meta_to_qtype(meta))?;
        // version
        self.w32(meta.st_mtime() as u32 ^ (meta.st_size() << 8) as u32)?;
        // path
        self.w64(meta.st_ino())
    }

    pub fn write_qid_path_only(&mut self, ino: u64) -> io::Result<()> {
        self.w8(0)?;
        self.w32(0)?;
        self.w64(ino)
    }

    pub fn write_statl(&mut self, st: &Metadata) -> io::Result<()> {
        self.w64(P9_STATS_BASIC)?;
        self.write_qid(&st)?;
        self.w32(st.st_mode())?;
        self.w32(st.st_uid())?;
        self.w32(st.st_gid())?;
        self.w64(st.st_nlink())?;
        self.w64(st.st_rdev())?;
        self.w64(st.st_size())?;
        self.w64(st.st_blksize())?;
        self.w64(st.st_blocks())?;
        self.w64(st.st_atime() as u64)?;
        self.w64(st.st_atime_nsec() as u64)?;
        self.w64(st.st_mtime() as u64)?;
        self.w64(st.st_mtime_nsec() as u64)?;
        self.w64(st.st_ctime() as u64)?;
        self.w64(st.st_ctime_nsec() as u64)?;
        self.w64(0)?;
        self.w64(0)?;
        self.w64(0)?;
        self.w64(0)?;
        Ok(())
    }

    pub fn write_statfs(&mut self, statfs: StatFs) -> io::Result<()> {
        self.w32(statfs.f_type)?;
        self.w32(statfs.f_bsize)?;
        self.w64(statfs.f_blocks)?;
        self.w64(statfs.f_bfree)?;
        self.w64(statfs.f_bavail)?;
        self.w64(statfs.f_files)?;
        self.w64(statfs.f_ffree)?;
        self.w64(statfs.fsid)?;
        self.w32(statfs.f_namelen)?;
        Ok(())
    }

    pub fn r8(&mut self) -> io::Result<u8> {
        self.chain.read_u8()
    }

    pub fn r16(&mut self) -> io::Result<u16> {
        self.chain.read_u16::<LittleEndian>()
    }

    pub fn r32(&mut self) -> io::Result<u32> {
        self.chain.read_u32::<LittleEndian>()
    }

    pub fn r64(&mut self) -> io::Result<u64> {
        self.chain.read_u64::<LittleEndian>()
    }

    pub fn w8(&mut self, val: u8) -> io::Result<()> {
        self.chain.write_u8(val)
    }

    pub fn w16(&mut self, val: u16) -> io::Result<()> {
        self.chain.write_u16::<LittleEndian>(val)
    }

    pub fn w32(&mut self, val: u32) -> io::Result<()> {
        self.chain.write_u32::<LittleEndian>(val)
    }
    pub fn w64(&mut self, val: u64) -> io::Result<()> {
        self.chain.write_u64::<LittleEndian>(val)
    }
}


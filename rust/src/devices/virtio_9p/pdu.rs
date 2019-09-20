use std::io::{self,Read,Write};
use std::os::unix::ffi::OsStrExt;
use std::ffi::OsStr;

use libc;
use byteorder::{LittleEndian,ReadBytesExt,WriteBytesExt};

use crate::devices::virtio_9p::file::Qid;
use crate::memory::GuestRam;
use crate::virtio::Chain;

const P9_HEADER_LEN: usize = 7;
const P9_RLERROR: u8 = 7;

pub struct PduParser<'a> {
    memory: GuestRam,
    pub chain: &'a mut Chain,

    size: u32,
    cmd: u8,
    tag: u16,
    reply_start_addr: u64,
}

#[derive(Default,Debug)]
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
    const MODE: u32 = (1 << 0);      // 0x01
    const UID: u32 = (1 << 1);       // 0x02
    const GID: u32 = (1 << 2);       // 0x04
    const SIZE: u32 = (1 << 3);      // 0x08
    const ATIME: u32 = (1 << 4);     // 0x10
    const MTIME: u32 = (1 << 5);     // 0x20
    const CTIME: u32 = (1 << 6);     // 0x40
    const ATIME_SET: u32 = (1 << 7); // 0x80
    const MTIME_SET: u32 = (1 << 8); // 0x100
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
        self.reply_start_addr = self.chain.current_write_address(8)
            .ok_or(io::Error::from_raw_os_error(libc::EIO))?;

        // reserve header
        self.w32(0)?;
        self.w8(0)?;
        self.w16(0)?;
        Ok(())
    }

    fn error_code(err: io::Error) -> u32 {
        if let Some(errno) = err.raw_os_error() {
            return errno as u32;
        }
        let errno = match err.kind() {
            io::ErrorKind::NotFound => libc::ENOENT,
            io::ErrorKind::PermissionDenied => libc::EPERM,
            io::ErrorKind::ConnectionRefused => libc::ECONNREFUSED,
            io::ErrorKind::ConnectionReset => libc::ECONNRESET,
            io::ErrorKind::ConnectionAborted => libc::ECONNABORTED,
            io::ErrorKind::NotConnected => libc::ENOTCONN,
            io::ErrorKind::AddrInUse => libc::EADDRINUSE,
            io::ErrorKind::AddrNotAvailable => libc::EADDRNOTAVAIL,
            io::ErrorKind::BrokenPipe => libc::EPIPE,
            io::ErrorKind::AlreadyExists => libc::EEXIST,
            io::ErrorKind::WouldBlock => libc::EWOULDBLOCK,
            io::ErrorKind::InvalidInput => libc::EINVAL,
            io::ErrorKind::InvalidData => libc::EINVAL,
            io::ErrorKind::TimedOut => libc::ETIMEDOUT,
            io::ErrorKind::WriteZero => libc::EIO,
            io::ErrorKind::Interrupted => libc::EINTR,
            io::ErrorKind::Other => libc::EIO,
            io::ErrorKind::UnexpectedEof => libc::EIO,
            _ => libc::EIO,
        };
        return errno as u32;
    }

    pub fn bail_err(&mut self, error: io::Error) -> io::Result<()> {
        let errno = Self::error_code(error);
        self.write_err(errno)
    }

    pub fn write_err(&mut self, errno: u32) -> io::Result<()> {
        if self.reply_start_addr == 0 {
            self.read_done()?;
        }
        self.w32(errno)?;
        self._w32_at(0,P9_HEADER_LEN as u32 + 4);
        self._w8_at(4, P9_RLERROR);
        self._w16_at(5, self.tag);
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

    pub fn read_string_list(&mut self) -> io::Result<Vec<String>> {
        let count = self.r16()?;
        let mut strings = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let s = self.read_string()?;
            strings.push(s);
        }
        Ok(strings)
    }

    pub fn read_attr(&mut self) -> io::Result<P9Attr> {
        let mut attr = P9Attr::new();
        attr.parse(self)?;
        Ok(attr)
    }

    pub fn write_string(&mut self, str: &str) -> io::Result<()> {
        let bytes = str.as_bytes();
        self.w16(bytes.len() as u16)?;
        self.chain.write_all(bytes)
    }

    pub fn write_os_string(&mut self, str: &OsStr) -> io::Result<()> {
        self.w16(str.len() as u16)?;
        self.chain.write_all(str.as_bytes())
    }

    pub fn write_qid_list(&mut self, list: &[Qid]) -> io::Result<()> {
        self.w16(list.len() as u16)?;
        for qid in list {
            qid.write(self)?;
        }
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

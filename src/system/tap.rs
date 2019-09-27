use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd,RawFd};

use crate::system;
use crate::system::ioctl::{
    ioctl_with_ref, ioctl_with_val, ioctl_with_mut_ref
};

pub struct Tap {
    file: File,
    name: String,
}

const IFF_TAP: u16      = 0x0002;
const IFF_NO_PI: u16    = 0x1000;
const IFF_VNET_HDR: u16 = 0x4000;

const TAPTUN: u64 = 0x54;
const TUNSETIFF: libc::c_ulong = iow!(TAPTUN, 202, 4);
const TUNSETOFFLOAD: libc::c_ulong = iow!(TAPTUN, 208, 4);
const TUNSETVNETHDRSZ: libc::c_ulong = iow!(TAPTUN, 216, 4);

impl Tap {
    pub fn new_default() -> io::Result<Self> {
        Self::new("vmtap%d")
    }

    pub fn new(if_name: &str) -> io::Result<Self> {
        let file = Self::open_tun()?;
        let mut ifreq = IfReq::new(if_name);

        ifreq
            .set_flags(IFF_TAP | IFF_NO_PI| IFF_VNET_HDR)
            .ioctl_mut(&file, TUNSETIFF)?;

        let name = ifreq.name().to_string();
        let tap = Tap { file, name };

        Ok(tap)
    }

    fn open_tun() -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK|libc::O_CLOEXEC)
            .open("/dev/net/tun")
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_offload(&self, flags: libc::c_uint) -> io::Result<()> {
        unsafe {
            ioctl_with_val(self.file.as_raw_fd(), TUNSETOFFLOAD, flags.into())?;
        }
        Ok(())
    }

    pub fn set_vnet_hdr_size(&self, size: libc::c_int) -> io::Result<()> {
        unsafe {
            ioctl_with_ref(self.file.as_raw_fd(), TUNSETVNETHDRSZ, &size)?;
        }
        Ok(())
    }
}

impl Read for Tap {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl Write for Tap {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AsRawFd for Tap {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

#[repr(C)]
#[derive(Copy,Clone,Default)]
struct IfReq {
    pub ireqn: IrReqN,
    pub irequ: IfReqU,
}

impl IfReq {
    fn new(ifname: &str) -> Self {
        let ifname = ifname.as_bytes();
        assert!(ifname.len() < 16);
        let mut ifreq = Self::default();
        ifreq.ireqn.name[..ifname.len()]
            .copy_from_slice(ifname);
        ifreq
    }

    fn name(&self) -> &str {
        if let Some(idx) = self.ireqn.name.iter().position(|&b| b == 0) {
            ::std::str::from_utf8(&self.ireqn.name[..idx]).unwrap()
        } else {
            ""
        }
    }

    fn set_flags(&mut self, flags: u16) -> &mut Self {
        self.irequ.flags = flags;
        self
    }

    fn ioctl_mut<R: AsRawFd>(&mut self, fd: &R, request: libc::c_ulong) -> system::Result<()> {
        unsafe {
            ioctl_with_mut_ref(fd.as_raw_fd(), request, self)?;
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Copy,Clone,Default)]
struct IrReqN {
    name: [u8; 16],
}

#[repr(C)]
#[derive(Copy,Clone)]
union IfReqU {
    flags: u16,
    addr: libc::sockaddr,
    addr_in: libc::sockaddr_in,
    ifindex: u32,
    _align: [u64; 3],
}


impl Default for IfReqU {
    fn default() -> Self {
        IfReqU { _align: [0u64; 3]}
    }
}

use std::ffi::CString;
use std::io::SeekFrom;
use std::os::unix::io::{RawFd,AsRawFd};

use crate::system::{Result, FileDesc, errno_result};

use libc::{
    self, c_char, c_uint, c_int, c_long,SYS_memfd_create,
    MFD_CLOEXEC, MFD_ALLOW_SEALING, F_SEAL_GROW,F_SEAL_SHRINK, F_SEAL_SEAL
};


#[derive(Debug)]
pub struct MemoryFd {
    fd: FileDesc,
    size: usize,
}

impl MemoryFd {

    pub fn from_filedesc(fd: FileDesc) -> Result<MemoryFd> {
        let size = fd.seek(SeekFrom::End(0))? as usize;
        Ok(MemoryFd { fd, size })
    }

    pub fn new_memfd(size: usize, sealed: bool) -> Result<MemoryFd> {
        Self::new_memfd_with_name(size, sealed, "pH-memfd")
    }

    pub fn new_memfd_with_name(size: usize, sealed: bool, name: &str) -> Result<MemoryFd> {
        let fd = Self::memfd_create(name, MFD_CLOEXEC | MFD_ALLOW_SEALING)?;
        fd.set_size(size)?;

        let memfd = MemoryFd { fd, size };
        if sealed {
            memfd.add_seals(F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_SEAL)?;
        }

        Ok(memfd)
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn fd_mut(&mut self) -> &mut FileDesc {
        &mut self.fd
    }

    fn memfd_create(name: &str, flags: c_uint) -> Result<FileDesc> {
        let name = CString::new(name).expect("Cstring from &str");
        let name = name.as_ptr() as *const c_char;
        let fd = unsafe { libc::syscall(SYS_memfd_create as c_long, name, flags) } as c_int;
        if fd < 0 {
            errno_result()
        } else {
            Ok(FileDesc::new(fd))
        }
    }

    fn add_seals(&self, flags: c_int) -> Result<()> {
        let ret = unsafe { libc::fcntl(self.fd.as_raw_fd(), libc::F_ADD_SEALS, flags) };
        if ret < 0 {
            errno_result()
        } else {
            Ok(())
        }
    }

}

impl AsRawFd for MemoryFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

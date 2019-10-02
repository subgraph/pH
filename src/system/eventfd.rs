use std::os::unix::io::{RawFd,AsRawFd};

use libc;

use crate::system::{Result,Error};

pub struct EventFd(RawFd);

const U64_SZ: usize = 8;

impl EventFd {
    pub fn new() -> Result<EventFd> {
        let fd = unsafe { libc::eventfd(0, 0) };
        if fd < 0 {
            return Err(Error::last_os_error());
        }
        Ok(EventFd(fd))
    }

    pub fn write(&self, v: u64) -> Result<()> {
        let ret = unsafe { libc::write(self.0, &v as *const _ as *const libc::c_void, U64_SZ) };
        if ret as usize != U64_SZ {
            if ret < 0 {
                return Err(Error::last_os_error())
            }
            return Err(Error::EventFdWrite);
        }
        Ok(())
    }

    pub fn read(&self) -> Result<u64> {
        let mut v = 0u64;
        let ret = unsafe { libc::read(self.0, &mut v as *mut _ as *mut libc::c_void, U64_SZ) };
        if ret as usize != U64_SZ {
            if ret < 0 {
                return Err(Error::last_os_error());
            }
            return Err(Error::EventFdRead);
        }
        Ok(v)
    }
}

impl Drop for EventFd {
    fn drop(&mut self) {
        let _ = unsafe { libc::close(self.0) };
    }
}

impl AsRawFd for EventFd  {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

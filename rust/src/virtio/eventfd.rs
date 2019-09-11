use std::sync::Arc;
use std::os::unix::io::{RawFd,AsRawFd};

use libc;

use crate::vm::{Result,Error,ErrorKind};
use crate::kvm::Kvm;

pub struct EventFd(RawFd);

const U64_SZ: usize = 8;

impl EventFd {
    pub fn new() -> Result<EventFd> {
        let fd = unsafe { libc::eventfd(0, 0) };
        if fd < 0 {
            return Err(Error::from_last_errno());
        }
        Ok(EventFd(fd))
    }

    pub fn write(&self, v: u64) -> Result<()> {
        let ret = unsafe { libc::write(self.0, &v as *const _ as *const libc::c_void, U64_SZ) };
        if ret as usize != U64_SZ {
            if ret < 0 {
                return Err(Error::new(ErrorKind::EventFdError, Error::from_last_errno()));
            }
            return Err(Error::new(ErrorKind::EventFdError, "write failed"));
        }
        Ok(())
    }

    pub fn read(&self) -> Result<u64> {
        let mut v = 0u64;
        let ret = unsafe { libc::read(self.0, &mut v as *mut _ as *mut libc::c_void, U64_SZ) };
        if ret as usize != U64_SZ {
            if ret < 0 {
                return Err(Error::new(ErrorKind::EventFdError, Error::from_last_errno()));
            }
            return Err(Error::new(ErrorKind::EventFdError, "read failed"));
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

pub struct IoEventFd {
    kvm: Kvm,
    addr: u64,
    evt: Arc<EventFd>
}

impl IoEventFd {
    pub fn new(kvm: &Kvm, address: u64) -> Result<IoEventFd> {
        let evt = Arc::new(EventFd::new()?);
        kvm.ioeventfd_add(address, evt.as_raw_fd())?;
        Ok(IoEventFd {
            kvm: kvm.clone(),
            addr: address,
            evt,
        })
    }
    pub fn read(&self) -> Result<u64> {
        self.evt.read()
    }

    pub fn write(&self, v: u64) -> Result<()> {
        self.evt.write(v)
    }
}

impl Drop for IoEventFd {
    fn drop(&mut self) {
        let _ = self.kvm.ioeventfd_del(self.addr, self.evt.as_raw_fd());
    }
}

impl AsRawFd for IoEventFd {
    fn as_raw_fd(&self) -> RawFd {
        self.evt.as_raw_fd()
    }
}

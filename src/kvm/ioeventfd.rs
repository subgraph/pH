use std::sync::Arc;
use std::os::unix::io::{AsRawFd,RawFd};

use crate::kvm::{Kvm,Result,Error};
use crate::system::EventFd;
use crate::system;

pub struct IoEventFd {
    kvm: Kvm,
    addr: u64,
    evt: Arc<EventFd>
}

impl IoEventFd {
    pub fn new(kvm: &Kvm, address: u64) -> Result<IoEventFd> {
        let evt = EventFd::new().map_err(Error::IoEventCreate)?;
        kvm.ioeventfd_add(address, evt.as_raw_fd())?;
        Ok(IoEventFd {
            kvm: kvm.clone(),
            addr: address,
            evt: evt.into(),
        })
    }
    pub fn read(&self) -> system::Result<u64> {
        self.evt.read()
    }

    pub fn write(&self, v: u64) -> system::Result<()> {
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

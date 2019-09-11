use std::os::unix::io::{RawFd,AsRawFd};
use std::ptr;
use crate::system::{Result,Error};
use std::time::Duration;

use libc::{epoll_event, c_int, EPOLLIN, EPOLLHUP, EPOLL_CTL_DEL, EPOLL_CTL_ADD, EPOLL_CLOEXEC, EINTR, EINVAL};

const MAX_EVENTS: usize = 32;

pub struct Event(epoll_event);

impl Event {
    pub fn id(&self) -> u64 {
        self.0.u64
    }
    pub fn is_readable(&self) -> bool {
        self.is_event(EPOLLIN)
    }

    pub fn is_hangup(&self) -> bool {
        self.is_event(EPOLLHUP)
    }

    fn is_event(&self, flag: c_int) -> bool {
        self.events() & flag as u32 != 0
    }

    pub fn events(&self) -> u32 {
        self.0.events
    }
}

pub struct EPoll {
    fd: RawFd,
    events: PollEvents,
}

impl EPoll {
    pub fn new() -> Result<EPoll> {
        match unsafe { libc::epoll_create1(EPOLL_CLOEXEC) } {
            -1 => Err(Error::last_os_error()),
            fd => Ok(EPoll {
                fd,
                events: PollEvents::new(),
            })
        }
    }

    pub fn add_read(&self, fd: RawFd, id: u64) -> Result<()> {
        let mut evt = epoll_event {
            events: EPOLLIN as u32,
            u64: id
        };
        match unsafe { libc::epoll_ctl(self.fd, EPOLL_CTL_ADD, fd, &mut evt) } {
            -1 => Err(Error::last_os_error()),
            _ => Ok(()),
        }
    }

    pub fn delete(&self, fd: RawFd) -> Result<()> {
        match unsafe { libc::epoll_ctl(self.fd, EPOLL_CTL_DEL, fd, ptr::null_mut()) } {
            -1 => Err(Error::last_os_error()),
            _ => Ok(()),
        }
    }

    pub fn wait_timeout(&mut self,timeout: Duration) -> Result<PollEvents> {
        let ms = timeout.as_millis() as u32;
        self.wait_ms(ms as c_int)
    }

    pub fn wait(&mut self) -> Result<PollEvents> {
        self.wait_ms(-1)
    }

    fn wait_ms(&mut self, timeout: c_int) -> Result<PollEvents> {
        let mut events = PollEvents::new();
        let nevents = events.len() as c_int;
        loop {
            let ret = unsafe {
                libc::epoll_wait(self.fd, events.events_ptr(), nevents, timeout)
            };

            if ret == -1 && Error::last_os_error() != Error::from_raw_os_error(EINTR) {
                return Err(Error::last_os_error());
            } else if ret as usize > events.len() {
                return Err(Error::from_raw_os_error(EINVAL));
            } else if ret != -1 {
                events.count = ret as usize;
                return Ok(events)
            }
        }
    }
}

impl AsRawFd for EPoll {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl Drop for EPoll {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd); }
    }
}

#[derive(Clone)]
pub struct PollEvents {
    count: usize,
    events: [epoll_event; MAX_EVENTS],
}

impl PollEvents {
    fn new() -> Self {
        PollEvents {
            count: 0,
            events: [epoll_event{ events: 0, u64: 0 }; MAX_EVENTS],
        }
    }
    pub fn iter(&self) -> PollEventIter {
        PollEventIter::new(&self.events[..self.count])
    }

    fn events_ptr(&mut self) -> *mut epoll_event {
        self.events.as_mut_ptr()
    }

    fn len(&self) -> usize {
        self.events.len()
    }
}

pub struct PollEventIter<'a> {
    idx: usize,
    events: &'a [epoll_event],
}

impl <'a> PollEventIter<'a> {
    fn new(events: &'a [epoll_event]) -> Self {
        PollEventIter {
            idx: 0,
            events,
        }
    }
}

impl <'a> Iterator for PollEventIter<'a> {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == self.events.len() {
            None
        } else {
            let ev = self.events[self.idx].clone();
            self.idx += 1;
            Some(Event(ev))
        }
    }
}



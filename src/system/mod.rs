#[macro_use]pub mod ioctl;
mod epoll;
mod errno;
mod eventfd;
mod socket;
mod filedesc;
mod memfd;
mod tap;
pub mod netlink;

pub use filedesc::{FileDesc, FileFlags};
pub use eventfd::EventFd;
pub use memfd::MemoryFd;
pub use epoll::{EPoll,Event};
pub use socket::ScmSocket;
pub use netlink::NetlinkSocket;
pub use tap::Tap;
use std::{fmt, result, io};

pub use errno::Error as ErrnoError;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Errno(errno::Error),
    OpenKvmFailed(errno::Error),
    InvalidOffset,
    InvalidAddress(u64),
    IoctlError(&'static str, errno::Error),
    EventFdWrite,
    EventFdRead,

}

impl Error {
    pub fn last_os_error() -> Error {
        Error::Errno(errno::Error::last_os_error())
    }

    pub fn last_errno() -> i32 {
        errno::Error::last_errno()
    }

    pub fn from_raw_os_error(e: i32) -> Error {
        Error::Errno(errno::Error::from_raw_os_error(e))
    }

    pub fn inner_err(&self) -> Option<&errno::Error> {
        match self {
            Error::IoctlError(_,e) => Some(e),
            Error::Errno(e) => Some(e),
            Error::OpenKvmFailed(e) => Some(e),
            _ => None,
        }
    }

    pub fn is_interrupted(&self) -> bool {
        self.inner_err()
            .map(|e| e.is_interrupted())
            .unwrap_or(false)
    }
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            Errno(err) => err.fmt(f),
            InvalidOffset => write!(f, "attempt to access invalid offset into mapping"),
            InvalidAddress(addr) => write!(f, "attempt to access invalid address: {0:16x}", addr),
            OpenKvmFailed(err) => write!(f, "failed to open /dev/kvm: {}", err),
            IoctlError(name, err) => write!(f, "failed to call {} ioctl: {}", name, err),
            EventFdWrite => write!(f, "failed writing to eventfd"),
            EventFdRead => write!(f, "failed reading from eventfd"),
        }
    }
}
impl From<errno::Error> for Error {
    fn from(err: errno::Error) -> Error {
        Error::Errno(err)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::from_raw_os_error(e.raw_os_error().unwrap_or_default())
    }
}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Errno(e) => io::Error::from_raw_os_error(e.errno()),
            e => io::Error::new(io::ErrorKind::Other, e),
        }
    }
}

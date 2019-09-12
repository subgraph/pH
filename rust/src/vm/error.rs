use std::{result, io};
use std::error;
use std::fmt;
use std::str;
use std::ffi::CStr;
use libc;
use crate::disk;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum ErrorKind {
    KernelNotFound,
    InitNotFound,
    InvalidAddress(u64),
    InvalidMappingOffset(usize),
    RegisterMemoryFailed,
    ReadKernelFailed,
    Interrupted,
    InvalidVring,
    IoctlFailed(&'static str),
    MissingRequiredExtension(u32),
    OpenDeviceFailed,
    CreateVmFailed,
    BadVersion,
    EventFdError,
    DiskImageOpen(disk::Error),
    TerminalTermios(io::Error),
}

impl ErrorKind {
    fn as_str(&self) -> &'static str {
        match *self {
            ErrorKind::KernelNotFound => "Could not find kernel image",
            ErrorKind::InitNotFound => "Could not find init image",
            ErrorKind::InvalidAddress(..) => "Invalid guest memory address",
            ErrorKind::InvalidMappingOffset(..) => "Invalid memory mapping offset",
            ErrorKind::RegisterMemoryFailed => "Failed to register memory region",
            ErrorKind::ReadKernelFailed => "Failed to load kernel from disk",
            ErrorKind::Interrupted => "System call interrupted",
            ErrorKind::InvalidVring => "Invalid Vring",
            ErrorKind::IoctlFailed(..) => "Ioctl failed",
            ErrorKind::MissingRequiredExtension(..) => "kernel does not support requred kvm extension",
            ErrorKind::OpenDeviceFailed => "could not open /dev/kvm",
            ErrorKind::CreateVmFailed => "call to create vm failed",
            ErrorKind::BadVersion => "unexpected kvm api version",
            ErrorKind::EventFdError => "eventfd error",
            ErrorKind::DiskImageOpen(_) => "failed to open disk image",
            ErrorKind::TerminalTermios(_) => "failed termios",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ErrorKind::InvalidAddress(addr) => write!(f, "{}: 0x{:x}", self.as_str(), addr),
            ErrorKind::InvalidMappingOffset(offset) => write!(f, "{}: 0x{:x}", self.as_str(), offset),
            ErrorKind::IoctlFailed(name) => write!(f, "Ioctl {} failed", name),
            ErrorKind::DiskImageOpen(ref e) => write!(f, "failed to open disk image: {}", e),
            ErrorKind::TerminalTermios(ref e) => write!(f, "error reading/restoring terminal state: {}", e),
            _ => write!(f, "{}", self.as_str()),
        }
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error { repr: Repr::Simple(kind) }
    }
}

enum Repr {
    Errno(i32),
    Simple(ErrorKind),
    General(Box<General>),
}

#[derive(Debug)]
struct General {
    kind: ErrorKind,
    error: Box<dyn error::Error+Send+Sync>,
}

#[derive(Debug)]
pub struct Error {
    repr: Repr,
}

impl Error {
    pub fn new<E>(kind: ErrorKind, error: E) -> Error
        where E: Into<Box<dyn error::Error+Send+Sync>> {
        Self::_new(kind, error.into())
    }

    fn _new(kind: ErrorKind, error: Box<dyn error::Error+Send+Sync>) -> Error {
        Error {
            repr: Repr::General(Box::new(General{
                kind, error
            }))
        }
    }

    pub fn from_last_errno() -> Error {
        let errno = unsafe { *libc::__errno_location() };
        Error::from_errno(errno)
    }

    pub fn from_errno(errno: i32) -> Error {
        if errno == libc::EINTR {
            Error { repr: Repr::Simple(ErrorKind::Interrupted) }
        } else {
            Error { repr: Repr::Errno(errno) }
        }
    }

    pub fn is_interrupted(&self) -> bool {
        match self.repr {
            Repr::Simple(ErrorKind::Interrupted) => true,
            _ => false,
        }
    }
}

fn error_string(errno: i32) -> String {
    let mut buf = [0 as libc::c_char; 256];
    let p = buf.as_mut_ptr();
    unsafe {
        if libc::strerror_r(errno as libc::c_int, p, buf.len()) < 0 {
            panic!("strerror_r failed in error_string");
        }
        let p = p as *const _;
        str::from_utf8(CStr::from_ptr(p).to_bytes()).unwrap().to_owned()
    }
}

impl fmt::Debug for Repr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Repr::Errno(ref errno) =>
                f.debug_struct("Errno").field("errno", errno)
                    .field("message", &error_string(*errno)).finish(),
            Repr::General(ref c) => f.debug_tuple("General").field(c).finish(),
            Repr::Simple(ref kind) => f.debug_tuple("Kind").field(kind).finish(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.repr {
            Repr::Errno(errno) => {
                let detail = error_string(errno);
                write!(f, "{} (errno: {})", detail, errno)
            }
            Repr::General(ref c) => {
                write!(f, "{}: {}", c.kind, c.error)
            },
            Repr::Simple(ref kind) => kind.fmt(f),
        }
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match self.repr {
            Repr::Errno(..) => "Errno Error",
            Repr::Simple(ref kind) => kind.as_str(),
            Repr::General(ref c) => c.error.description(),
        }
    }

    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self.repr {
            Repr::Errno(..) => None,
            Repr::Simple(..) => None,
            Repr::General(ref c) => c.error.source(),
        }
    }
}


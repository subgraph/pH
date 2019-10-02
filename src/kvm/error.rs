use std::{fmt, result};

use crate::system::Error as SysError;
use crate::system::ErrnoError;
pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    OpenKvm(ErrnoError),
    MissingRequiredExtension(u32),
    BadVersion,
    IoctlError(&'static str, ErrnoError),
    IoEventCreate(SysError),
}

impl Error {
    pub fn is_interrupted(&self) -> bool {
        match self {
            Error::IoctlError(_, e) => e.is_interrupted(),
            _ => false,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            OpenKvm(e) => write!(f, "could not open /dev/kvm: {}", e),
            MissingRequiredExtension(ext) => write!(f, "kernel does not support a required kvm extension: {}", ext),
            BadVersion => write!(f, "unexpected kvm api version"),
            IoctlError(name, err) => write!(f, "failed to call {} ioctl: {}", name, err),
            IoEventCreate(e) => write!(f, "failed to create ioeventfd: {}", e),
        }
    }
}

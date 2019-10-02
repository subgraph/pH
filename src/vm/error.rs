use std::{result, io};
use std::fmt;
use crate::{system, kvm, virtio};
use crate::system::netlink;
use crate::vm::arch;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    CreateVmFailed(kvm::Error),
    MappingFailed(system::Error),
    TerminalTermios(io::Error),
    IoError(io::Error),
    ArchError(arch::Error),
    NetworkSetup(netlink::Error),
    SetupBootFs(io::Error),
    SetupVirtio(virtio::Error),
}


impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::TerminalTermios(e) => write!(f, "error reading/restoring terminal state: {}", e),
            Error::IoError(e) => write!(f, "i/o error: {}", e),
            Error::NetworkSetup(e) => write!(f, "error setting up network: {}", e),
            Error::CreateVmFailed(e) => write!(f, "call to create vm failed: {}", e),
            Error::MappingFailed(e) => write!(f, "memory mapping failed: {}", e),
            Error::SetupBootFs(e) => write!(f, "setting up boot fs failed: {}", e),
            Error::SetupVirtio(e) => write!(f, "setting up virtio devices failed: {}", e),
            Error::ArchError(e) => e.fmt(f),
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::IoError(err).into()
    }

}

impl From<netlink::Error> for Error {
    fn from(err: netlink::Error) -> Error {
        Error::NetworkSetup(err).into()
    }
}

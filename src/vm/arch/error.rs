use crate::{kvm, system, memory};
use crate::system::ErrnoError;
use std::{fmt, result};

#[derive(Debug)]
pub enum Error {
    MemoryManagerCreate(memory::Error),
    MemoryRegister(kvm::Error),
    MemoryRegionCreate(system::Error),
    LoadKernel(system::Error),
    KvmError(kvm::Error),
    SystemError(system::Error),
    IoctlError(&'static str, ErrnoError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            MemoryManagerCreate(err) => write!(f, "failed to create memory manager: {}", err),
            MemoryRegister(err) => write!(f, "failed to register memory region: {}", err),
            MemoryRegionCreate(err) => write!(f, "failed to create memory region: {}", err),
            LoadKernel(err) => write!(f, "error loading kernel: {}", err),
            KvmError(e) => e.fmt(f),
            SystemError(e) => e.fmt(f),
            IoctlError(name, err) => write!(f, "failed to call {} ioctl: {}", name, err),
        }
    }
}

pub type Result<T> = result::Result<T, Error>;

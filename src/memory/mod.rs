mod ram;
mod drm;
mod manager;
mod mmap;
mod address;
mod allocator;

pub use self::allocator::SystemAllocator;
pub use self::address::AddressRange;
pub use self::mmap::Mapping;
pub use self::ram::{GuestRam,MemoryRegion};
pub use manager::MemoryManager;

pub use drm::{DrmDescriptor,DrmPlaneDescriptor};

use std::{result, fmt, io};
use crate::{system, kvm};

#[derive(Debug)]
pub enum Error {
    DeviceMemoryAllocFailed,
    MappingFailed(system::Error),
    RegisterMemoryFailed(kvm::Error),
    UnregisterMemoryFailed(kvm::Error),
    GbmCreateDevice(system::Error),
    GbmCreateBuffer(system::Error),
    OpenRenderNode(io::Error),
    PrimeHandleToFD(system::ErrnoError),
    CreateBuffer(io::Error),
    NoDrmAllocator,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            DeviceMemoryAllocFailed => write!(f, "failed to allocate memory for device"),
            MappingFailed(e) => write!(f, "failed to create memory mapping for device memory: {}", e),
            RegisterMemoryFailed(e) => write!(f, "failed to register memory for device memory: {}", e),
            UnregisterMemoryFailed(e) => write!(f, "failed to unregister memory for device memory: {}", e),
            GbmCreateDevice(e) => write!(f, "failed to open device with libgbm: {}", e),
            GbmCreateBuffer(e) => write!(f, "failed to allocate buffer with libgbm: {}", e),
            PrimeHandleToFD(err) => write!(f, "exporting prime handle to fd failed: {}", err),
            OpenRenderNode(err) => write!(f, "error opening render node: {}", err),
            CreateBuffer(err) => write!(f, "failed to create buffer: {}", err),
            NoDrmAllocator => write!(f, "no DRM allocator is available"),
        }
    }
}

pub type Result<T> = result::Result<T, Error>;



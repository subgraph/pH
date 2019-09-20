mod ram;
mod manager;
mod mmap;
mod address;
mod allocator;

pub use self::allocator::SystemAllocator;
pub use self::address::AddressRange;
pub use self::mmap::Mapping;
pub use self::ram::GuestRam;
pub use self::ram::{PCI_MMIO_RESERVED_BASE,HIMEM_BASE};
pub use manager::MemoryManager;
use crate::vm::Error as VmError;
use std::{result, fmt};

pub const KVM_KERNEL_LOAD_ADDRESS: u64 = 0x1000000;
pub const KERNEL_CMDLINE_ADDRESS: u64 = 0x20000;
pub const KERNEL_ZERO_PAGE: u64 = 0x7000;

#[derive(Debug)]
pub enum Error {
    DeviceMemoryAllocFailed,
    MappingFailed(VmError),
    RegisterMemoryFailed(VmError),
    UnregisterMemoryFailed(VmError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            DeviceMemoryAllocFailed => write!(f, "failed to allocate memory for device"),
            MappingFailed(e) => write!(f, "failed to create memory mapping for device memory: {}", e),
            RegisterMemoryFailed(e) => write!(f, "failed to register memory for device memory: {}", e),
            UnregisterMemoryFailed(e) => write!(f, "failed to unregister memory for device memory: {}", e),
        }
    }
}

pub type Result<T> = result::Result<T, Error>;



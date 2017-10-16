mod ram;
mod mmap;
mod address;

pub use self::address::AddressRange;
pub use self::mmap::Mapping;
pub use self::ram::GuestRam;
pub use self::ram::{PCI_MMIO_RESERVED_BASE,HIMEM_BASE};

pub const KVM_KERNEL_LOAD_ADDRESS: u64 = 0x1000000;
pub const KERNEL_CMDLINE_ADDRESS: u64 = 0x20000;
pub const KERNEL_ZERO_PAGE: u64 = 0x7000;


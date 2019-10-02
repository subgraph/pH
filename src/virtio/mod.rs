mod bus;
mod chain;
mod config;
mod consts;
mod device;
mod pci;
mod virtqueue;
mod vring;
mod device_config;

pub use self::virtqueue::VirtQueue;
pub use self::pci::PciIrq;
pub use self::bus::VirtioBus;
pub use self::device::{VirtioDevice,VirtioDeviceOps};
pub use self::chain::Chain;
pub use self::device_config::DeviceConfigArea;

use byteorder::{ByteOrder,LittleEndian};
use std::{result, fmt};
use crate::{system, kvm};

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    CreateEventFd(system::Error),
    CreateIoEventFd(kvm::Error),
    ReadIoEventFd(system::Error),
    IrqFd(kvm::Error),
    VringNotEnabled,
    VringRangeInvalid(u64),
    VringAvailInvalid(u64),
    VringUsedInvalid(u64),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            CreateIoEventFd(e) => write!(f, "failed to create IoEventFd for VirtQueue: {}", e),
            CreateEventFd(e) => write!(f, "failed to create EventFd for VirtQueue: {}", e),
            ReadIoEventFd(e) => write!(f, "failed to read from IoEventFd: {}", e),
            IrqFd(e) => write!(f, "VirtQueue: {}", e),
            VringNotEnabled => write!(f, "vring is not enabled"),
            VringRangeInvalid(addr) => write!(f, "vring descriptor table range is invalid 0x{:x}", addr),
            VringAvailInvalid(addr) => write!(f, "vring avail ring range range is invalid 0x{:x}", addr),
            VringUsedInvalid(addr) => write!(f, "vring used ring range is invalid 0x{:x}", addr),

        }
    }
}

pub fn read_config_buffer(config: &[u8], offset: usize, size: usize) -> u64 {
    if offset + size > config.len() {
        return 0;
    }
    match size {
        1 => config[offset] as u64,
        2 => LittleEndian::read_u16(&config[offset..]) as u64,
        4 => LittleEndian::read_u32(&config[offset..]) as u64,
        8 => LittleEndian::read_u64(&config[offset..]) as u64,
        _ => 0,
    }
}

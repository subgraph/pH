mod bus;
mod chain;
mod config;
mod consts;
mod device;
mod eventfd;
mod pci;
mod virtqueue;
mod vring;
mod device_config;

pub use self::virtqueue::VirtQueue;
pub use self::pci::PciIrq;
pub use self::bus::VirtioBus;
pub use self::device::{VirtioDevice,VirtioDeviceOps};
pub use self::chain::Chain;
pub use self::eventfd::EventFd;
pub use self::device_config::DeviceConfigArea;

use byteorder::{ByteOrder,LittleEndian};

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

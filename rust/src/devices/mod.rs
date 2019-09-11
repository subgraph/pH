pub mod serial;
pub mod rtc;
mod virtio_9p;
mod virtio_serial;
mod virtio_rng;
mod virtio_wl;
mod virtio_block;

pub use self::virtio_serial::VirtioSerial;
pub use self::virtio_9p::VirtioP9;
pub use self::virtio_rng::VirtioRandom;
pub use self::virtio_wl::VirtioWayland;
pub use self::virtio_block::VirtioBlock;

pub mod serial;
pub mod rtc;
pub mod virtio_9p;
pub mod virtio_serial;
pub mod virtio_rng;
mod virtio_block;

pub use self::virtio_serial::VirtioSerial;
pub use self::virtio_9p::VirtioP9;
pub use self::virtio_rng::VirtioRandom;
pub use self::virtio_block::VirtioBlock;

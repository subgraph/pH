#[macro_use]
extern crate lazy_static;
#[macro_use]
mod system;
#[macro_use]
pub mod util;
mod vm;
mod memory;
mod devices;
mod kvm;
mod virtio;
mod disk;

pub use util::{Logger,LogLevel};
pub use vm::VmConfig;

#![allow(non_snake_case)]

#[macro_use] extern crate lazy_static;

#[macro_use] mod log;
mod vm;
#[macro_use]
mod system;
mod memory;
mod devices;
mod kvm;
mod virtio;
mod disk;

pub use log::{Logger,LogLevel};

fn main() {
    vm::VmConfig::new()
        .ram_size_megs(1024)
        .boot();
}

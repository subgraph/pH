#![allow(non_snake_case)]

#[macro_use] extern crate lazy_static;

#[macro_use] mod log;
mod vm;
mod memory;
#[macro_use]
mod system;
mod devices;
mod kvm;
mod virtio;
mod disk;

pub use log::{Logger,LogLevel};
use std::env;

fn main() {
    vm::VmConfig::new(env::args())
        .ram_size_megs(1024)
        .use_realmfs("/home/user/Shared/main-realmfs.img")
        .boot();
}

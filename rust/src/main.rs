#![allow(non_snake_case)]

extern crate libc;
extern crate byteorder;
extern crate termios;


mod vm;
mod memory;
#[macro_use]
mod system;
mod devices;
mod kvm;
mod virtio;


use std::env;
use std::path::PathBuf;
fn main() {

    let mut config = vm::VmConfig::new();
    config.ram_size_megs(1024);
    match find_kernel() {
        Some(path) => config.kernel_path(&path),
        None => { println!("Could not find kernel"); return; }
    }
    match find_init() {
        Some(path) => config.init_path(&path),
        None => { println!("Could not find init"); return; }
    }
    match vm::Vm::open(config) {
        Ok(vm) => {
            vm.start().unwrap();
        },
        Err(e) => println!("error :( {}", e)
    }
}

fn find_init() -> Option<PathBuf> {
    let mut cwd = env::current_dir().unwrap();
    if cwd.join("rust/target/release/ph-init").exists() {
        cwd.push("rust/target/release/ph-init");
        return Some(cwd)
    }
    if cwd.join("rust/target/debug/ph-init").exists() {
        cwd.push("rust/target/debug/ph-init");
        return Some(cwd)
    }
    None
}

fn find_kernel() -> Option<PathBuf> {
    let mut cwd = env::current_dir().unwrap();
    if cwd.join("kernel/ph_linux").exists() {
        cwd.push("kernel/ph_linux");
        return Some(cwd)
    }
    None
}

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
use std::path::{Path,PathBuf};
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
    match find_kernel_base() {
        Some(buf) => Some(buf.join("init/init")),
        None => None,
    }
}
fn find_kernel() -> Option<PathBuf> {
    match find_kernel_base() {
        Some(buf) => Some(buf.join("build/linux-4.9.56/vmlinux")),
        None => None,
    }
}

fn find_kernel_base() -> Option<PathBuf> {
    let mut cwd = env::current_dir().unwrap();
    if try_kernel_base(&cwd) {
        cwd.push("kernel");
        return Some(cwd);
    }
    None
}

fn try_kernel_base(path: &Path) -> bool {
    path.join("kernel/build/linux-4.9.56/vmlinux").exists()
}

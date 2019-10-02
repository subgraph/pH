#![allow(non_snake_case)]

use ph::VmConfig;

fn main() {
    VmConfig::new()
        .ram_size_megs(2048)
        .boot();
}

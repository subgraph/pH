
use std::sync::{Arc,RwLock};
use std::thread;
use std::fs::File;

use crate::virtio::{VirtioDeviceOps,VirtioBus,VirtQueue};
use crate::memory::GuestRam;
use crate::vm::Result;


const VIRTIO_ID_RANDOM: u16 = 4;

pub struct VirtioRandom;

impl VirtioRandom {
    fn new() -> VirtioRandom { VirtioRandom }

    pub fn create(vbus: &mut VirtioBus) -> Result<()> {
        let dev = Arc::new(RwLock::new(VirtioRandom::new()));
        vbus.new_virtio_device(VIRTIO_ID_RANDOM, dev)
            .set_num_queues(1)
            .register()
    }
}

impl VirtioDeviceOps for VirtioRandom {

    fn start(&mut self, _memory: GuestRam, mut queues: Vec<VirtQueue>) {
        thread::spawn(move|| {
            run(queues.pop().unwrap())
        });
    }
}

fn run(q: VirtQueue) {
    let random = File::open("/dev/urandom").unwrap();

    loop {
        q.on_each_chain(|mut chain| {
            while !chain.is_end_of_chain() {
                let _ = chain.copy_from_reader(&random, 256).unwrap();
            }
        });
    }
}
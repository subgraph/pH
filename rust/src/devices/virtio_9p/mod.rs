use std::sync::{Arc,RwLock};
use std::thread;

use std::path::{Path,PathBuf};

use crate::memory::GuestRam;
use crate::virtio::{self,VirtioBus,VirtioDeviceOps, VirtQueue};
use crate::vm::Result;

mod fid;
mod pdu;
mod commands;
mod readdir;
mod filesystem;

use self::pdu::PduParser;
use self::commands::Commands;

const VIRTIO_ID_9P: u16 = 9;
const VIRTIO_9P_MOUNT_TAG: u64 = 0x1;


pub struct VirtioP9 {
    root_dir: PathBuf,
    init_path: PathBuf,
    feature_bits: u64,
    config: Vec<u8>,
}

impl VirtioP9 {
    fn create_config(tag_name: &str) -> Vec<u8> {
        let tag_len = tag_name.len() as u16;
        let mut config = Vec::with_capacity(tag_name.len() + 3);
        config.push(tag_len as u8);
        config.push((tag_len >> 8) as u8);
        config.append(&mut tag_name.as_bytes().to_vec());
        config.push(0);
        config
    }

    fn new(tag_name: &str, root_dir: &str, init_path: &Path) -> Arc<RwLock<VirtioP9>> {
        Arc::new(RwLock::new(VirtioP9 {
            root_dir: PathBuf::from(root_dir),
            init_path: init_path.to_path_buf(),
            feature_bits: 0,
            config: VirtioP9::create_config(tag_name),
        }))
    }

    pub fn create(vbus: &mut VirtioBus, tag_name: &str, root_dir: &str, init_path: &Path) -> Result<()> {
        vbus.new_virtio_device(VIRTIO_ID_9P, VirtioP9::new(tag_name, root_dir, init_path))
            .set_num_queues(1)
            .set_features(VIRTIO_9P_MOUNT_TAG)
            .set_config_size(tag_name.len() + 3)
            .register()
    }
}

impl VirtioDeviceOps for VirtioP9 {
    fn reset(&mut self) {
        println!("Reset called");
    }

    fn enable_features(&mut self, bits: u64) -> bool {
        self.feature_bits = bits;
        true
    }

    fn read_config(&mut self, offset: usize, size: usize) -> u64 {
        virtio::read_config_buffer(&self.config, offset, size)
    }


    fn start(&mut self, memory: GuestRam, mut queues: Vec<VirtQueue>) {
        let vq = queues.pop().unwrap();
        let root_dir = self.root_dir.clone();
        let init_path = self.init_path.clone();
        thread::spawn(|| run_device(memory, vq, root_dir, init_path));
    }
}

fn run_device(memory: GuestRam, vq: VirtQueue, root_dir: PathBuf, init_path: PathBuf) {
    let mut commands = Commands::new(root_dir,init_path,memory.clone());

    vq.on_each_chain(|mut chain| {
        let mut pp = PduParser::new(&mut chain, memory.clone());
        commands.handle(&mut pp);
    });
}


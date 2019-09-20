use std::sync::{Arc,RwLock};
use std::thread;

use std::path::{PathBuf, Path};

use crate::memory::{GuestRam, MemoryManager};
use crate::virtio::{self,VirtioBus,VirtioDeviceOps, VirtQueue};
use crate::vm::Result;
use crate::devices::virtio_9p::server::Server;
use crate::devices::virtio_9p::filesystem::{FileSystem, FileSystemOps};
use self::pdu::PduParser;

mod pdu;
mod file;
mod directory;
mod filesystem;
mod server;
mod synthetic;


const VIRTIO_ID_9P: u16 = 9;
const VIRTIO_9P_MOUNT_TAG: u64 = 0x1;

pub use synthetic::SyntheticFS;

pub struct VirtioP9<T: FileSystemOps> {
    filesystem: T,
    root_dir: PathBuf,
    feature_bits: u64,
    debug: bool,
    config: Vec<u8>,
}

impl <T: FileSystemOps+'static> VirtioP9<T> {
    fn create_config(tag_name: &str) -> Vec<u8> {
        let tag_len = tag_name.len() as u16;
        let mut config = Vec::with_capacity(tag_name.len() + 3);
        config.push(tag_len as u8);
        config.push((tag_len >> 8) as u8);
        config.append(&mut tag_name.as_bytes().to_vec());
        config.push(0);
        config
    }

    fn new(filesystem: T, tag_name: &str, root_dir: &str, debug: bool) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(VirtioP9 {
            filesystem,
            root_dir: PathBuf::from(root_dir),
            feature_bits: 0,
            debug,
            config: VirtioP9::<T>::create_config(tag_name),
        }))
    }

    pub fn create_with_filesystem(filesystem: T, vbus: &mut VirtioBus, tag_name: &str, root_dir: &str, debug: bool) -> Result<()> {
        vbus.new_virtio_device(VIRTIO_ID_9P, VirtioP9::new(filesystem, tag_name, root_dir, debug))
            .set_num_queues(1)
            .set_features(VIRTIO_9P_MOUNT_TAG)
            .set_config_size(tag_name.len() + 3)
            .register()
    }
}

impl VirtioP9<FileSystem> {

    pub fn create(vbus: &mut VirtioBus, tag_name: &str, root_dir: &str, read_only: bool, debug: bool) -> Result<()> {
        let filesystem = FileSystem::new(PathBuf::from(root_dir), read_only);
        Self::create_with_filesystem(filesystem, vbus, tag_name, root_dir, debug)
    }
}

impl <T: FileSystemOps+'static> VirtioDeviceOps for VirtioP9<T> {
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

    fn start(&mut self, memory: &MemoryManager, mut queues: Vec<VirtQueue>) {
        let vq = queues.pop().unwrap();
        let root_dir = self.root_dir.clone();
        let filesystem = self.filesystem.clone();
        let ram = memory.guest_ram().clone();
        let debug = self.debug;
        thread::spawn(move || run_device(ram, vq, &root_dir, filesystem, debug));
    }
}

fn run_device<T: FileSystemOps>(memory: GuestRam, vq: VirtQueue, root_dir: &Path, filesystem: T, debug: bool) {
    let mut server = Server::new(&root_dir, filesystem);

    if debug {
        server.enable_debug();
    }

    vq.on_each_chain(|mut chain| {
        let mut pp = PduParser::new(&mut chain, memory.clone());
        server.handle(&mut pp);
    });
}


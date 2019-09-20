use crate::{vm, disk};
use crate::virtio::{VirtioBus, VirtioDeviceOps, VirtQueue, DeviceConfigArea, Chain};
use std::sync::{RwLock, Arc};
use crate::memory::MemoryManager;
use std::{result, io, fmt, thread};
use crate::devices::virtio_block::Error::IoChainError;
use std::io::Write;
use crate::disk::DiskImage;

const VIRTIO_BLK_F_RO: u64 = (1 << 5);
//const VIRTIO_BLK_F_BLK_SIZE: u64 = (1 << 6);
const VIRTIO_BLK_F_FLUSH: u64 = (1 << 9);
//const VIRTIO_BLK_F_DISCARD: u64 = (1 << 13);
//const VIRTIO_BLK_F_WRITE_ZEROES: u64 = (1 << 14);

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_T_FLUSH: u32 = 4;
const VIRTIO_BLK_T_GET_ID: u32 = 8;
//const VIRTIO_BLK_T_DISCARD: u32 = 11;
//const VIRTIO_BLK_T_WRITE_ZEROES: u32 = 13;

const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_IOERR: u8 = 1;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

const SECTOR_SIZE: usize = 512;

// TODO:
//   - feature bits
//   - disk image write overlay
//   - better error handling for i/o
enum Error {
    IoChainError(io::Error),
    DiskRead(disk::Error),
    DiskWrite(disk::Error),
    DiskFlush(disk::Error),
    VirtQueueWait(vm::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        IoChainError(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            IoChainError(e) => write!(f, "i/o error on virtio chain operation: {}", e),
            DiskRead(e) => write!(f, "error reading disk image: {}", e),
            DiskWrite(e) => write!(f, "error writing disk image: {}", e),
            DiskFlush(e) => write!(f, "error flushing disk image: {}", e),
            VirtQueueWait(e) =>write!(f, "error waiting on virtqueue: {}", e),
        }
    }
}
type Result<T> = result::Result<T, Error>;

pub struct VirtioBlock<D: DiskImage+'static> {
    disk_image: Option<D>,
    config: DeviceConfigArea,
    enabled_features: u64,
}

const VIRTIO_ID_BLOCK: u16 = 2;
impl <D: DiskImage + 'static> VirtioBlock<D> {
    pub fn new(disk_image: D) -> Self {
        let mut config = DeviceConfigArea::new(8);
        config.write_u64(0, disk_image.sector_count());
        VirtioBlock {
            disk_image: Some(disk_image),
            config,
            enabled_features: 0,
        }
    }

    pub fn create(vbus: &mut VirtioBus, disk_image: D) -> vm::Result<()> {
        let feature_bits = if disk_image.read_only() {
            VIRTIO_BLK_F_FLUSH|VIRTIO_BLK_F_RO
        } else {
            VIRTIO_BLK_F_FLUSH
        };

        let dev = Arc::new(RwLock::new(VirtioBlock::new(disk_image)));

        vbus.new_virtio_device(VIRTIO_ID_BLOCK, dev)
            .set_queue_sizes(&[256])
            .set_config_size(8)
            .set_features(feature_bits)
            .register()
    }
}

impl <D: DiskImage> VirtioDeviceOps for VirtioBlock<D> {
    fn enable_features(&mut self, bits: u64) -> bool {
        self.enabled_features = bits;
        true
    }

    fn write_config(&mut self, offset: usize, size: usize, val: u64) {
        self.config.write_config(offset, size, val);
    }

    fn read_config(&mut self, offset: usize, size: usize) -> u64 {
        self.config.read_config(offset, size)
    }

    fn start(&mut self, _: &MemoryManager, mut queues: Vec<VirtQueue>) {
        let vq = queues.pop().unwrap();
        let mut dev = match self.disk_image.take() {
            Some(d) => VirtioBlockDevice::new(vq, d),
            None => {
                warn!("Unable to start virtio-block device. Already started?");
                return;
            }
        };

        thread::spawn(move || {
            if let Err(err) = dev.run() {
                warn!("Error running virtio block device: {}", err);
            }
        });

    }
}

struct VirtioBlockDevice<D: DiskImage> {
    vq: VirtQueue,
    disk: D,
}

impl <D: DiskImage> VirtioBlockDevice<D> {
    fn new(vq: VirtQueue, disk: D) -> Self {
        VirtioBlockDevice { vq, disk }
    }

    fn run(&mut self) -> Result<()> {
        loop {
            let chain = self.vq.wait_next_chain()
                .map_err(Error::VirtQueueWait)?;

            match MessageHandler::read_header(&mut self.disk, chain) {
                Ok(mut handler) => handler.process_message(),
                Err(e) => {
                    warn!("Error handling virtio_block message: {}", e);
                }
            }
        }
    }
}

struct MessageHandler<'a, D: DiskImage> {
    disk: &'a mut D,
    chain: Chain,
    msg_type: u32,
    sector: u64,
}

impl <'a, D: DiskImage> MessageHandler<'a, D> {

    fn read_header(disk: &'a mut D, mut chain: Chain) -> Result<Self> {
        let msg_type = chain.r32()?;
        let _ = chain.r32()?;
        let sector = chain.r64()?;
        Ok(MessageHandler { disk, chain, msg_type, sector })
    }

    fn process_message(&mut self)  {
        let r = match self.msg_type {
            VIRTIO_BLK_T_IN => self.handle_io_in(),
            VIRTIO_BLK_T_OUT => self.handle_io_out(),
            VIRTIO_BLK_T_FLUSH => self.handle_io_flush(),
            VIRTIO_BLK_T_GET_ID => self.handle_get_id(),
            cmd => {
                warn!("virtio_block: unexpected command: {}", cmd);
                self.write_status(VIRTIO_BLK_S_UNSUPP);
                Ok(())
            },
        };
        self.process_result(r);
    }

    fn process_result(&mut self, result: Result<()>) {
        match result {
            Ok(()) => self.write_status(VIRTIO_BLK_S_OK),
            Err(e) => {
                warn!("virtio_block: disk error: {}", e);
                self.write_status(VIRTIO_BLK_S_IOERR);
            }
        }
    }

    fn sector_round(sz: usize) -> usize {
        (sz / SECTOR_SIZE) * SECTOR_SIZE
    }

    fn handle_io_in(&mut self) -> Result<()> {
        let current = self.chain.current_write_slice();
        let len = Self::sector_round(current.len());
        let buffer = &mut current[..len];

        self.disk.read_sectors(self.sector, buffer)
            .map_err(Error::DiskRead)?;
        self.chain.inc_offset(len, true);
        Ok(())
    }

    fn handle_io_out(&mut self) -> Result<()> {
        let current = self.chain.current_read_slice();
        let len = Self::sector_round(current.len());
        let buffer = &current[..len];

        self.disk.write_sectors(self.sector, buffer)
            .map_err(Error::DiskWrite)?;
        self.chain.inc_offset(len, false);
        Ok(())
    }

    fn handle_io_flush(&mut self) -> Result<()> {
        self.disk.flush().map_err(Error::DiskFlush)
    }

    fn handle_get_id(&mut self) -> Result<()> {
        self.chain.write_all(self.disk.disk_image_id())?;
        Ok(())
    }

    fn write_status(&mut self, status: u8) {
        if let Err(e) = self.chain.w8(status) {
           warn!("Error writing block device status: {}", e);
        }
        self.chain.flush_chain();
    }
}
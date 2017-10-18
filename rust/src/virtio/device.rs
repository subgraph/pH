use std::sync::{Arc,RwLock};
use std::ops::DerefMut;

use memory::{GuestRam,AddressRange};
use super::bus::VirtioDeviceConfig;
use super::VirtQueue;
use super::config::VirtQueueConfig;
use super::consts::*;
use vm::io::MmioOps;
use vm::Result;

pub trait VirtioDeviceOps: Send+Sync {
    fn reset(&mut self) {}
    fn enable_features(&mut self, bits: u64) -> bool { let _ = bits; true }
    fn write_config(&mut self, offset: usize, size: usize, val: u64) { let (_,_,_) = (offset, size, val); }
    fn read_config(&mut self, offset: usize, size: usize) -> u64 { let (_,_) = (offset, size); 0 }
    fn start(&mut self, memory: GuestRam, queues: Vec<VirtQueue>);
}

pub struct VirtioDevice {
    memory: GuestRam,
    vq_config: VirtQueueConfig,
    common_cfg_mmio: AddressRange,
    isr_mmio: AddressRange,
    notify_mmio: AddressRange,
    device_cfg_mmio: Option<AddressRange>,
    device_ops: Arc<RwLock<VirtioDeviceOps>>,
    dfselect: u32,
    gfselect: u32,
    device_features: u64,
    guest_features: u64,
    status: u8,
}

const MASK_LOW_32: u64 = (1u64 << 32) - 1;
const MASK_HI_32: u64 = MASK_LOW_32 << 32;

fn set_lo32(val: &mut u64, low32: u32)  { *val = (*val & MASK_HI_32) | (low32 as u64) }
fn set_hi32(val: &mut u64, hi32: u32)  { *val = ((hi32 as u64) << 32) | (*val & MASK_LOW_32) }
fn get_lo32(val: u64) -> u32 { val as u32 }
fn get_hi32(val: u64) -> u32 { (val >> 32) as u32 }



impl VirtioDevice {
    pub fn new(memory: GuestRam, config: &VirtioDeviceConfig) -> Result<Arc<RwLock<VirtioDevice>>> {
        Ok(Arc::new(RwLock::new(VirtioDevice {
            memory: memory.clone(),
            vq_config: VirtQueueConfig::new(&memory.clone(),&config)?,
            common_cfg_mmio: config.common_cfg_mmio(),
            isr_mmio: config.isr_mmio(),
            notify_mmio: config.notify_mmio(),
            device_cfg_mmio: config.device_cfg_mmio(),

            device_ops: config.ops(),
            dfselect: 0,
            gfselect: 0,

            device_features: config.feature_bits(),
            guest_features: 0,
            status: 0,
        })))
    }

    fn reset(&mut self) {
        self.dfselect = 0;
        self.gfselect = 0;
        self.guest_features = 0;
        self.status = 0;
        self.vq_config.reset();
    }

    fn status_write(&mut self, val: u8) {

        // 4.1.4.3.1 The device MUST reset when 0 is written to device status
        if val == 0 {
            self.reset();
            return;
        }
        // 2.1.1 The driver MUST NOT clear a device status bit
        if self.status & !val != 0 {
            return;
        }

        let new_bits = val & !self.status;

        if new_bits & VIRTIO_CONFIG_S_DRIVER_OK != 0 {
            match self.vq_config.create_queues(&self.memory) {
                Ok(queues) => self.with_ops(|ops| ops.start(self.memory.clone(), queues)),
                Err(e) => {
                    println!("creating virtqueues failed {}", e);
                    self.status |= VIRTIO_CONFIG_S_NEEDS_RESET;
                    self.vq_config.notify_config();
                    return;
                }
            }
        }

        if new_bits & VIRTIO_CONFIG_S_FEATURES_OK != 0 {
            if !self.with_ops(|ops| ops.enable_features(self.guest_features)) {
                self.vq_config.enable_features(self.guest_features);
               return;
            }
        }

        self.status |= new_bits;
    }

    fn common_config_write(&mut self, offset: usize, _size: usize, val: u32) {
        match offset {
            VIRTIO_PCI_COMMON_DFSELECT => self.dfselect = val,
            VIRTIO_PCI_COMMON_GFSELECT => self.gfselect = val,
            VIRTIO_PCI_COMMON_GF => {
                match self.gfselect {
                    0 => set_lo32(&mut self.guest_features, val),
                    1 => set_hi32(&mut self.guest_features, val),
                    _ => {},
                }
                // 2.2.1
                //   The driver MUST NOT accept a feature which the device did
                //   not offer.
                self.guest_features &= self.device_features;
            },
            VIRTIO_PCI_COMMON_STATUS => self.status_write(val as u8),
            VIRTIO_PCI_COMMON_Q_SELECT=> self.vq_config.select_queue(val as u16),
            VIRTIO_PCI_COMMON_Q_SIZE => self.vq_config.vring_set_size(val as u16),
            VIRTIO_PCI_COMMON_Q_ENABLE=> if val == 1 { self.vq_config.vring_enable() } ,
            VIRTIO_PCI_COMMON_Q_DESCLO=> self.vq_config.with_vring_mut(|vr| set_lo32(&mut vr.descriptors, val)),
            VIRTIO_PCI_COMMON_Q_DESCHI=> self.vq_config.with_vring_mut(|vr| set_hi32(&mut vr.descriptors, val)),
            VIRTIO_PCI_COMMON_Q_AVAILLO=> self.vq_config.with_vring_mut(|vr| set_lo32(&mut vr.avail_ring, val)),
            VIRTIO_PCI_COMMON_Q_AVAILHI=> self.vq_config.with_vring_mut(|vr| set_hi32(&mut vr.avail_ring, val)),
            VIRTIO_PCI_COMMON_Q_USEDLO=> self.vq_config.with_vring_mut(|vr| set_lo32(&mut vr.used_ring, val)),
            VIRTIO_PCI_COMMON_Q_USEDHI=> self.vq_config.with_vring_mut(|vr| set_hi32(&mut vr.used_ring, val)),
            _ => {},
        }
    }

    fn common_config_read(&mut self, offset: usize, _size: usize) -> u32 {
        match offset {
            VIRTIO_PCI_COMMON_DFSELECT => self.dfselect,
            VIRTIO_PCI_COMMON_DF=> match self.dfselect {
                0 => get_lo32(self.device_features),
                1 => get_hi32(self.device_features),
                _ => 0,
            },
            VIRTIO_PCI_COMMON_GFSELECT => { self.gfselect },
            VIRTIO_PCI_COMMON_GF => match self.gfselect {
                0 => get_lo32(self.guest_features),
                1 => get_hi32(self.guest_features),
                _ => 0,
            },
            VIRTIO_PCI_COMMON_MSIX => VIRTIO_NO_MSI_VECTOR as u32,
            VIRTIO_PCI_COMMON_NUMQ => self.vq_config.num_queues() as u32,
            VIRTIO_PCI_COMMON_STATUS => self.status as u32,
            VIRTIO_PCI_COMMON_CFGGENERATION => 0,
            VIRTIO_PCI_COMMON_Q_SELECT => self.vq_config.selected_queue() as u32,
            VIRTIO_PCI_COMMON_Q_SIZE => self.vq_config.vring_get_size() as u32,
            VIRTIO_PCI_COMMON_Q_MSIX => VIRTIO_NO_MSI_VECTOR as u32,
            VIRTIO_PCI_COMMON_Q_ENABLE => if self.vq_config.vring_is_enabled() {1} else {0},
            VIRTIO_PCI_COMMON_Q_NOFF => self.vq_config.selected_queue() as u32,
            VIRTIO_PCI_COMMON_Q_DESCLO => self.vq_config.with_vring(0, |vr| get_lo32(vr.descriptors)),
            VIRTIO_PCI_COMMON_Q_DESCHI => self.vq_config.with_vring(0, |vr| get_hi32(vr.descriptors)),
            VIRTIO_PCI_COMMON_Q_AVAILLO => self.vq_config.with_vring(0, |vr| get_lo32(vr.avail_ring)),
            VIRTIO_PCI_COMMON_Q_AVAILHI => self.vq_config.with_vring(0, |vr| get_hi32(vr.avail_ring)),
            VIRTIO_PCI_COMMON_Q_USEDLO => self.vq_config.with_vring(0, |vr| get_lo32(vr.used_ring)),
            VIRTIO_PCI_COMMON_Q_USEDHI => self.vq_config.with_vring(0, |vr| get_hi32(vr.used_ring)),
            _ => 0,
        }
    }

    fn notify_read(&mut self, _offset: usize, _size: usize) -> u64 {
        0
    }

    fn notify_write(&mut self, offset: usize, _size: usize, _val: u64) {
        let vq = (offset / 4) as u16;
        self.vq_config.notify(vq);
    }

    fn isr_read(&mut self) -> u64 {
        self.vq_config.isr_read()
    }

    fn with_ops<U,F>(&self, f: F) -> U
      where F: FnOnce(&mut VirtioDeviceOps) -> U {
        let mut ops = self.device_ops.write().unwrap();
        f(ops.deref_mut())
    }
}

impl MmioOps for VirtioDevice {
    fn mmio_read(&mut self, address: u64, size: usize) -> u64 {
        if self.common_cfg_mmio.contains(address, size) {
            let offset = self.common_cfg_mmio.offset_of(address);
            self.common_config_read(offset,size) as u64

        } else if self.notify_mmio.contains(address, size) {
            let offset = self.notify_mmio.offset_of(address);
            self.notify_read(offset, size) as u64

        } else if self.isr_mmio.contains(address, size) {
            self.isr_read()

        } else if let Some(ref dev_cfg_mmio) = self.device_cfg_mmio {
            let offset = dev_cfg_mmio.offset_of(address);
            self.with_ops(|ops| ops.read_config(offset, size))

        } else {
            0
        }
    }

    fn mmio_write(&mut self, address: u64, size: usize, val: u64) {
        if self.common_cfg_mmio.contains(address, size) {
            let offset = self.common_cfg_mmio.offset_of(address);
            self.common_config_write(offset,size, val as u32)

        } else if self.notify_mmio.contains(address, size) {
            let offset = self.notify_mmio.offset_of(address);
            self.notify_write(offset, size, val)

        } else if let Some(ref dev_cfg_mmio) = self.device_cfg_mmio {
            let offset = dev_cfg_mmio.offset_of(address);
            self.with_ops(|ops| ops.write_config(offset, size, val))
        }
    }
}


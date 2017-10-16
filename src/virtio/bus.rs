use std::sync::{Arc,RwLock};
use vm::io::IoDispatcher;
use kvm::Kvm;
use memory::{GuestRam,AddressRange};
use super::{VirtioDevice,VirtioDeviceOps,PciIrq};
use super::consts::*;
use super::pci::PciBus;
use vm::Result;


pub struct VirtioBus {
    kvm: Kvm,
    memory: GuestRam,
    io_dispatcher: Arc<IoDispatcher>,
    pci_bus: Arc<RwLock<PciBus>>,
    devices: Vec<Arc<RwLock<VirtioDevice>>>,
}

impl VirtioBus {
    pub fn new(memory: GuestRam, io_dispatcher: Arc<IoDispatcher>, kvm: Kvm) -> VirtioBus {
        VirtioBus {
            kvm,
            memory,
            io_dispatcher: io_dispatcher.clone(),
            pci_bus: PciBus::new(&io_dispatcher),
            devices: Vec::new(),
        }
    }

    pub fn new_virtio_device(&mut self, device_type: u16, ops: Arc<RwLock<VirtioDeviceOps>>) -> VirtioDeviceConfig {
        VirtioDeviceConfig::new(self, device_type, ops)
    }

    pub fn pci_irqs(&self) -> Vec<PciIrq> {
        self.pci_bus.read().unwrap().pci_irqs()
    }
}

pub struct VirtioDeviceConfig<'a> {
    virtio_bus: &'a mut VirtioBus,
    device_type: u16,
    irq: u8,
    kvm: Kvm,
    ops: Arc<RwLock<VirtioDeviceOps>>,
    mmio: AddressRange,
    num_queues: usize,
    config_size: usize,
    device_class: u16,
    features: u64,

}

impl <'a> VirtioDeviceConfig<'a> {
    fn new(virtio_bus: &mut VirtioBus, device_type: u16, ops: Arc<RwLock<VirtioDeviceOps>>) -> VirtioDeviceConfig {
        let kvm = virtio_bus.kvm.clone();
        let mmio = virtio_bus.pci_bus.write().unwrap().allocate_mmio_space(VIRTIO_MMIO_AREA_SIZE);
        VirtioDeviceConfig {
            virtio_bus,
            device_type,
            irq: 0,
            kvm,
            ops,
            mmio,
            num_queues: 0,
            config_size: 0,
            features: 0,
            device_class: 0x0880,
        }
    }

    pub fn kvm(&self) -> &Kvm { &self.kvm }

    pub fn ops(&self) -> Arc<RwLock<VirtioDeviceOps>> {
        self.ops.clone()
    }
    pub fn irq(&self) -> u8 { self.irq }

    pub fn common_cfg_mmio(&self) -> AddressRange {
        self.mmio.subrange(VIRTIO_MMIO_OFFSET_COMMON_CFG, VIRTIO_MMIO_COMMON_CFG_SIZE).unwrap()
    }

    pub fn notify_mmio(&self) -> AddressRange {
        self.mmio.subrange(VIRTIO_MMIO_OFFSET_NOTIFY, VIRTIO_MMIO_NOTIFY_SIZE).unwrap()
    }

    pub fn isr_mmio(&self) -> AddressRange {
        self.mmio.subrange(VIRTIO_MMIO_OFFSET_ISR, VIRTIO_MMIO_ISR_SIZE).unwrap()
    }

    pub fn device_cfg_mmio(&self) -> Option<AddressRange> {
        if self.config_size > 0 {
            Some(self.mmio.subrange(VIRTIO_MMIO_OFFSET_DEV_CFG, self.config_size).unwrap())
        } else {
            None
        }
    }

    pub fn feature_bits(&self) -> u64 {
        self.features
    }

    pub fn num_queues(&self) -> usize {
        self.num_queues
    }

    #[allow(dead_code)]
    pub fn config_size(&self) -> usize {
        self.config_size
    }

    pub fn set_num_queues(&mut self, n: usize) -> &'a mut VirtioDeviceConfig {
        self.num_queues = n;
        self
    }

    pub fn set_config_size(&mut self, sz: usize) -> &'a mut VirtioDeviceConfig {
        self.config_size = sz;
        self
    }

    pub fn set_device_class(&mut self, cl: u16) -> &'a mut VirtioDeviceConfig {
        self.device_class = cl;
        self
    }

    pub fn set_features(&mut self, features: u64) -> &'a mut VirtioDeviceConfig {
        self.features = features;
        self
    }

    pub fn register(&mut self) -> Result<()> {
        self.create_pci_device();
        self.features |= VIRTIO_F_VERSION_1;
        //self.features |= VIRTIO_F_EVENT_IDX;
        let dev = VirtioDevice::new(self.virtio_bus.memory.clone(), &self)?;
        self.virtio_bus.io_dispatcher.register_mmio(self.mmio, dev.clone());
        self.virtio_bus.devices.push(dev);
        Ok(())
    }

    fn create_pci_device(&mut self) {
        let mut pci_bus = self.virtio_bus.pci_bus.write().unwrap();
        let mut pci = pci_bus.create_device(PCI_VENDOR_ID_REDHAT, PCI_VIRTIO_DEVICE_ID_BASE + self.device_type, self.device_class);
        pci.add_virtio_caps(self.config_size);
        pci.set_mmio_bar(VIRTIO_MMIO_BAR, self.mmio);
        self.irq = pci.get_irq();
        pci_bus.store_device(pci);
    }
}
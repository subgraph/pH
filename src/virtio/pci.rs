use std::sync::{Arc,RwLock};
use byteorder::{ByteOrder,LittleEndian};

use crate::vm::io::{IoDispatcher,IoPortOps};
use crate::vm::arch::PCI_MMIO_RESERVED_BASE;
use crate::memory::AddressRange;
use super::consts::*;

struct PciConfigAddress(u32);

impl PciConfigAddress {
    fn new() -> PciConfigAddress { PciConfigAddress(0) }
    fn set(&mut self, n: u32) { self.0 = n }
    fn get(&self) -> u32 { self.0 }
    fn bus(&self) -> u32 { self.bits(16, 8) }
    fn function(&self) -> u32 { self.bits(8, 3) }
    fn device(&self) -> usize { self.bits(11, 5) as usize }
    fn offset(&self) -> usize { (self.bits(0, 8) & !0x3) as usize }
    fn bits(&self, offset: u32, size: u32) -> u32 {
        let mask = (1u32 << size) - 1;
        (self.0 >> offset) & mask
    }
}

pub struct PciIrq {
    pci_id: u8,
    int_pin: u8,
    irq: u8,
}

impl PciIrq {
    fn new(pci: &PciDevice) -> PciIrq {
        PciIrq {
            pci_id: pci.id,
            int_pin: 1,
            irq: pci.irq,
        }
    }

    pub fn src_bus_irq(&self) -> u8 {
        (self.pci_id << 2) | (self.int_pin - 1)
    }

    pub fn irq_line(&self) -> u8 {
        self.irq
    }
}

pub struct PciBus {
    devices: Vec<Option<PciDevice>>,
    mmio_next_alloc: u32,
    next_irq: u8,
    next_dev: u8,
    config_address: PciConfigAddress,
}

impl PciBus {
    pub fn new(io: &IoDispatcher) -> Arc<RwLock<PciBus>> {
        let bus = Arc::new(RwLock::new(PciBus {
            devices: PciBus::create_device_vec(PCI_MAX_DEVICES),
            mmio_next_alloc: PCI_MMIO_RESERVED_BASE as u32,
            next_irq: 5,
            next_dev: 1,
            config_address: PciConfigAddress::new(),
        }));

        io.register_ioports(PCI_CONFIG_ADDRESS, 8, bus.clone());
        let pci = PciDevice::new(0, 0, PCI_VENDOR_ID_INTEL, 0, PCI_CLASS_BRIDGE_HOST);
        bus.write().unwrap().store_device(pci);
        bus
    }

    pub fn pci_irqs(&self) -> Vec<PciIrq> {
        let mut v = Vec::new();
        for d in &self.devices {
            match *d {
                Some(ref dev) => v.push(PciIrq::new(dev)),
                None => (),
            }
        }
        v
    }

    fn allocate_irq(&mut self) -> u8 {
        let irq = self.next_irq;
        self.next_irq += 1;
        irq
    }

    fn allocate_id(&mut self) -> u8 {
        let id = self.next_dev;
        self.next_dev += 1;
        id
    }

    pub fn create_device(&mut self, vendor: u16, device: u16, class_id: u16) -> PciDevice {
        let irq = self.allocate_irq();
        let id = self.allocate_id();
        let pci = PciDevice::new(id, irq, vendor, device, class_id);
        pci
    }

    pub fn store_device(&mut self, pci: PciDevice) {
        let id = pci.id as usize;
        self.devices[id] = Some(pci)
    }

    fn create_device_vec(sz: usize) -> Vec<Option<PciDevice>> {
        let mut v = Vec::with_capacity(sz);
        for _ in 0..sz {
            v.push(None)
        }
        v
    }

    pub fn allocate_mmio_space(&mut self, sz: usize) -> AddressRange {
        let mask = (sz - 1) as u32;
        let aligned = (self.mmio_next_alloc + mask) & !mask;
        self.mmio_next_alloc = aligned + (sz as u32);
        AddressRange::new(aligned as u64, sz)
    }

    fn is_in_range(base: u16, port: u16, len: usize) -> bool {
        let end = port + len as u16;
        port >= base && end <= (base + 4)
    }

    fn is_config_address(&self, port: u16, len: usize) -> bool {
        PciBus::is_in_range(PCI_CONFIG_ADDRESS, port, len)
    }

    fn is_config_data(&self, port: u16, len: usize) -> bool {
        PciBus::is_in_range(PCI_CONFIG_DATA, port, len)
    }

    fn config_address_in(&self, _: usize) -> u32 {
        self.config_address.get()
    }

    fn current_config_device(&mut self) -> Option<&mut PciDevice> {
        let b = self.config_address.bus();
        let d = self.config_address.device();
        let f = self.config_address.function();

        if b != 0 || f != 0 || d >= self.devices.len() {
            return None;
        }

        self.devices[d].as_mut()
    }

    fn config_address_out(&mut self, _offset: u16, size: usize, data: u32) {
        if size == 4 {
            self.config_address.set(data);
        }
    }

    #[allow(dead_code)]
    fn valid_config_access(&self, offset: u16, len: usize) -> bool {
        (offset as usize) + len <= 4
    }

    fn config_data_in(&mut self, offset: usize, size: usize) -> u32 {
        let off = self.config_address.offset() + offset;
        match self.current_config_device() {
            Some(dev) => { dev.read_config(off, size)},
            None => 0xFFFFFFFF,
        }
    }

    fn config_data_out(&mut self, offset: u16, size: usize, data: u32) {
        let off = self.config_address.offset() + offset as usize;
        if let Some(dev) = self.current_config_device() {
            dev.write_config(off, size,data)
        }
    }
}

impl IoPortOps for PciBus {
    fn io_in(&mut self, port: u16, size: usize) -> u32 {
        if self.is_config_address(port, size) {
            return self.config_address_in(size)
        }
        if self.is_config_data(port, size) {
            return self.config_data_in((port - PCI_CONFIG_DATA) as usize, size)
        }
        return 0;
    }

    fn io_out(&mut self, port: u16, size: usize, val: u32) {
        if self.is_config_address(port, size) {
            self.config_address_out(port - PCI_CONFIG_ADDRESS,size,  val)
        }
        if self.is_config_data(port, size) {
            self.config_data_out(port - PCI_CONFIG_DATA, size, val)
        }
    }
}


pub struct PciDevice {
    next_cap: usize,
    last_cap: usize,
    id: u8,
    irq: u8,
    config_buffer: [u8; PCI_CONFIG_SPACE_SIZE],
    bar_write_masks: [u32; 6],
}

impl PciDevice {
    pub fn new(id: u8, irq: u8, vendor: u16, device: u16, class_id: u16) -> PciDevice {
        let mut d = PciDevice {
            next_cap: PCI_CAP_BASE_OFFSET,
            last_cap: 0,
            id,
            irq,
            config_buffer: [0; PCI_CONFIG_SPACE_SIZE],
            bar_write_masks: [0; 6],
        };
        d.w16(PCI_VENDOR_ID, vendor);
        d.w16(PCI_DEVICE_ID, device);
        d.w16(PCI_COMMAND, PCI_COMMAND_IO | PCI_COMMAND_MEMORY);
        d.w8(PCI_CLASS_REVISION, 1);
        d.w16(PCI_CLASS_DEVICE, class_id);
        d.w8(PCI_INTERRUPT_PIN, 1);
        d.w8(PCI_INTERRUPT_LINE, irq);
        d.w16(PCI_SUBSYSTEM_ID, 0x40);
        d
    }

    pub fn get_irq(&self) -> u8 {
        self.irq
    }

    fn is_valid_write(&self, offset: usize, size: usize) -> bool {
        if offset + size > PCI_CONFIG_SPACE_SIZE {
            return false;
        }
        // check alignment of write
        let mod4 = offset % 4;
        match size {
            4 if mod4 == 0 => true,
            2 if mod4 == 0 || mod4 == 2 => true,
            1 => true,
            _ => false,
        }
    }

    fn write_bar(&mut self, offset: usize, size: usize, data: u32) {
        assert!(is_bar_offset(offset), "not a bar offset in write_bar()");

        let bar = offset_to_bar(offset);
        let write_mask = self.bar_write_masks[bar];

        if write_mask == 0 {
            // no writable bits
            return;
        }

        let mod4 = offset % 4;

        match size {
            4 => self.w32(offset, data),
            2 => self.w16(offset+ mod4, data as u16),
            1 => self.w8(offset+ mod4, data as u8),
            _ => (),
        };

        // apply write mask to whatever was written
        let v = self.r32(offset);
        self.w32(offset, v & write_mask);
    }

    fn write_config(&mut self, offset: usize, size: usize, data: u32) {
        if !self.is_valid_write(offset, size) {
            return;
        }

        if is_bar_offset(offset) {
            self.write_bar(offset, size, data);
            return;
        }

        match offset {
            PCI_COMMAND if size == 2 => self.w16(PCI_COMMAND, data as u16),
            PCI_STATUS if size == 2 => self.w16(PCI_STATUS, data as u16),
            PCI_CACHE_LINE_SIZE if size == 1 => self.w8(PCI_CACHE_LINE_SIZE, data as u8),
            PCI_LATENCY_TIMER if size == 1 => self.w8(PCI_LATENCY_TIMER, data as u8),
            _ => (),
        }
    }

    fn w32(&mut self, off: usize, val: u32) { LittleEndian::write_u32(&mut self.config_buffer[off..], val); }
    fn w16(&mut self, off: usize, val: u16) { LittleEndian::write_u16(&mut self.config_buffer[off..], val); }
    fn w8(&mut self, off: usize, val: u8) { self.config_buffer[off] = val; }

    fn r32(&self, off: usize) -> u32 { LittleEndian::read_u32(&self.config_buffer[off..]) }
    fn r16(&self, off: usize) -> u16 { LittleEndian::read_u16(&self.config_buffer[off..]) }
    fn r8(&self, off: usize) -> u8 { self.config_buffer[off] }

    fn read_config(&self, offset: usize, size: usize) -> u32 {
        if offset + size > PCI_CONFIG_SPACE_SIZE {
            return 0xFFFFFFFF;
        }
        match size {
            1 => self.r8(offset) as u32,
            2 => self.r16(offset) as u32,
            4 => self.r32(offset),
            _ => 0xFFFFFFFF
        }
    }

    #[allow(dead_code)]
    pub fn is_irq_disabled(&self) -> bool {
        self.r16(PCI_COMMAND) & PCI_COMMAND_INTX_DISABLE != 0
    }

    pub fn set_mmio_bar(&mut self, bar: usize, range: AddressRange) {
        assert!(range.is_naturally_aligned(), "cannot set_mmio_bar() because mmio range is not naturally aligned");
        assert!(bar < 5, "bar is invalid value in set_mmio_bar()");
        self.bar_write_masks[bar] = !((range.size() as u32) - 1);
        self.w32(bar_to_offset(bar), range.base() as u32);
    }

    pub fn add_virtio_caps(&mut self, config_size: usize) {
        self.new_virtio_cap(VIRTIO_PCI_CAP_COMMON_CFG)
            .set_mmio_range(VIRTIO_MMIO_OFFSET_COMMON_CFG, VIRTIO_MMIO_COMMON_CFG_SIZE).add(self);

        self.new_virtio_cap(VIRTIO_PCI_CAP_ISR_CFG)
            .set_mmio_range(VIRTIO_MMIO_OFFSET_ISR, VIRTIO_MMIO_ISR_SIZE).add(self);

        self.new_virtio_cap(VIRTIO_PCI_CAP_NOTIFY_CFG)
            .set_mmio_range(VIRTIO_MMIO_OFFSET_NOTIFY, VIRTIO_MMIO_NOTIFY_SIZE)
            .set_extra_word(4).add(self);

        if config_size > 0 {
            self.new_virtio_cap(VIRTIO_PCI_CAP_DEVICE_CFG)
                .set_mmio_range(VIRTIO_MMIO_OFFSET_DEV_CFG,config_size).add(self);
        }
    }

    pub fn new_virtio_cap(&mut self, vtype: u8) -> VirtioCap {
        VirtioCap::new(self.next_cap, vtype)
    }

    fn inc_cap(&mut self, size: usize) {
        let next = self.next_cap as u8;
        let last = self.last_cap;
        if self.last_cap == 0 {
            self.w8(PCI_CAPABILITY_LIST, next);
            let status = self.r16(PCI_STATUS) | PCI_STATUS_CAP_LIST;
            self.w16(PCI_STATUS, status);
        } else {
            self.w8(last + 1, next);
        }
        self.last_cap = self.next_cap;
        let aligned = (size + 3) & !3;
        self.next_cap += aligned;
    }
}

fn is_bar_offset(offset: usize) -> bool {
    offset >= 0x10 && offset < 0x28
}

fn bar_to_offset(bar: usize) -> usize {
    0x10 + (bar * 4)
}

fn offset_to_bar(offset: usize) -> usize {
    assert!(offset >= 0x10 && offset < 0x28, "not a valid bar offset");
    (offset - 0x10) / 4
}


pub struct VirtioCap {
    offset: usize,
    vtype: u8,
    size: u8,
    mmio_offset: u32,
    mmio_len: u32,
    extra_word: Option<u32>,
}

impl VirtioCap {
    fn new(offset: usize, vtype: u8) -> VirtioCap {
        VirtioCap {
            vtype,
            offset,
            size: 16,
            mmio_offset: 0,
            mmio_len: 0,
            extra_word: None,
        }
    }

    pub fn set_mmio_range(&mut self, offset: usize, len: usize) -> &mut VirtioCap {
        self.mmio_offset = offset as u32;
        self.mmio_len = len as u32;
        self
    }

    pub fn set_extra_word(&mut self, val: u32) -> &mut VirtioCap {
        self.size += 4;
        self.extra_word = Some(val);
        self
    }

    pub fn add(&mut self, dev: &mut PciDevice) {
        /*
         * struct virtio_pci_cap {
         *     u8 cap_vndr; /* Generic PCI field: PCI_CAP_ID_VNDR */
         *     u8 cap_next; /* Generic PCI field: next ptr. */
         *     u8 cap_len; /* Generic PCI field: capability length */
         *     u8 cfg_type; /* Identifies the structure. */
         *     u8 bar; /* Where to find it. */
         *     u8 padding[3]; /* Pad to full dword. */
         *     le32 offset; /* Offset within bar. */
         *     le32 length; /* Length of the structure, in bytes. */
         * };
         */
        dev.w8(self.offset, PCI_CAP_ID_VENDOR);
        dev.w8(self.offset + 2, self.size);
        dev.w8(self.offset + 3, self.vtype);
        dev.w8(self.offset + 4, VIRTIO_MMIO_BAR as u8);
        if self.mmio_len > 0 {
            dev.w32(self.offset + 8, self.mmio_offset);
            dev.w32(self.offset + 12, self.mmio_len);
        }
        if let Some(word) = self.extra_word {
            dev.w32(self.offset + 16, word);
        }

        dev.inc_cap(self.size as usize);
    }
}

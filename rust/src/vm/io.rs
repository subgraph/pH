use std::sync::{Arc,RwLock,RwLockWriteGuard};
use memory::AddressRange;

pub trait IoPortOps: Send+Sync {
    fn io_in(&mut self, port: u16, size: usize) -> u32 {
        let (_,_) = (port, size);
        0
    }
    fn io_out(&mut self, port: u16, size: usize, val: u32) {
        let (_,_,_) = (port,size,val);
    }
}

pub trait MmioOps: Send+Sync {
    fn mmio_read(&mut self, address: u64, size: usize) -> u64 {
        let (_,_)  = (address, size);
        0
    }

    fn mmio_write(&mut self, address: u64, size: usize, val: u64) {
        let (_,_,_) = (address, size, val);
    }
}

struct IoPortDummy;

impl IoPortOps for IoPortDummy {}

struct IoPortPS2Control;

impl IoPortOps for IoPortPS2Control {
    fn io_in(&mut self, _port: u16, _size: usize) -> u32 { 0x02 }
}

struct IoPortFakeI8042(bool);

impl IoPortOps for IoPortFakeI8042 {
    fn io_in(&mut self, port: u16, _size: usize) -> u32 {
        if port == 0x64 {
            0x1
        } else if port == 0x61 {
            0x20
        } else {
            0
        }
    }
    fn io_out(&mut self, port: u16, _size: usize, val: u32) {
        if port == 0x64 && val == 0xfe && !self.0 {
            self.0 = true;
            println!("Reset signal!");
        }
    }
}

struct IoPortEntry {
    port: u16,
    count: usize,
    device: Arc<RwLock<IoPortOps>>,
}

impl IoPortEntry {
    fn new(port: u16, count: usize, device: Arc<RwLock<IoPortOps>>) -> IoPortEntry {
        IoPortEntry{ port: port, count: count, device: device }
    }

    fn contains(&self, port: u16) -> bool {
        port >= self.port && port < (self.port + self.count as u16)
    }

    fn io_in(&mut self, port: u16, size: usize) -> u32 {
        let mut d = self.device.write().unwrap();
        d.io_in(port, size)
    }

    fn io_out(&mut self, port: u16, size: usize, val: u32) {
        let mut d = self.device.write().unwrap();
        d.io_out(port, size, val)
    }
}

struct MmioEntry {
    range: AddressRange,
    device: Arc<RwLock<MmioOps>>,
}

impl MmioEntry {
    fn new(range: AddressRange, device: Arc<RwLock<MmioOps>>) -> MmioEntry {
        MmioEntry{ range, device }
    }

    fn contains_range(&self, address: u64, length: usize) -> bool {
        self.range.contains(address, length)
    }

    fn read(&mut self, address: u64, size: usize) -> u64 {
        self.device.write().unwrap().mmio_read(address, size)
    }

    fn write(&mut self, address: u64, size: usize, val: u64) {
        self.device.write().unwrap().mmio_write(address, size, val)
    }
}


pub struct IoDispatcher {
    state: RwLock<IoDispatcherState>,
}

impl IoDispatcher {
    pub fn new() -> Arc<IoDispatcher> {
        Arc::new(IoDispatcher{
            state: RwLock::new(IoDispatcherState::new()),
        })
    }

    fn state_mut(&self) -> RwLockWriteGuard<IoDispatcherState> {
        self.state.write().unwrap()
    }

    pub fn register_ioports(&self, port: u16, count: usize, dev: Arc<RwLock<IoPortOps>>) {
        self.state_mut().register_ioports(port, count, dev)
    }

    pub fn register_mmio(&self, range: AddressRange, device: Arc<RwLock<MmioOps>>) {
        self.state_mut().register_mmio(range, device);
    }

    pub fn emulate_io_in(&self, port: u16, size: usize) -> u32 {
        self.state_mut().emulate_io_in(port, size)

    }
    pub fn emulate_io_out(&self, port: u16, size: usize, val: u32) {
        self.state_mut().emulate_io_out(port, size, val)
    }

    pub fn emulate_mmio_read(&self, address: u64, size: usize) -> u64 {
        self.state_mut().emulate_mmio_read(address, size)
    }

    pub fn emulate_mmio_write(&self, address: u64, size: usize, val: u64) {
        self.state_mut().emulate_mmio_write(address, size, val)
    }
}

struct IoDispatcherState {
    last_unhandled_port: u16,
    ioport_entries: Vec<IoPortEntry>,
    mmio_entries: Vec<MmioEntry>,
}

impl IoDispatcherState {
    pub fn new() -> IoDispatcherState {
        let mut st = IoDispatcherState {
            last_unhandled_port: 0,
            ioport_entries: Vec::new(),
            mmio_entries: Vec::new(),
        };
        st.setup_ioports();
        st
    }

    fn register_ioports(&mut self, port: u16, count: usize, dev: Arc<RwLock<IoPortOps>>) {
        self.ioport_entries.push(IoPortEntry::new(port, count, dev));
    }

    fn register_mmio(&mut self, range: AddressRange, device: Arc<RwLock<MmioOps>>) {
        self.mmio_entries.push(MmioEntry::new(range, device));
    }

    fn mmio_for(&mut self, address: u64, size: usize) -> Option<&mut MmioEntry> {
        for e in &mut self.mmio_entries {
            if e.contains_range(address, size) {
                return Some(e);
            }
        }
        None
    }

    fn ioports_for(&mut self, port: u16) -> Option<&mut IoPortEntry> {
        for e in &mut self.ioport_entries {
            if e.contains(port) {
                return Some(e);
            }
        }
        None
    }

    fn emulate_io_in(&mut self, port: u16, size: usize) -> u32 {
        if let Some(entry) = self.ioports_for(port) {
            return entry.io_in(port, size);
        }
        self.debug_port(port, true);
        0
    }

    fn emulate_io_out(&mut self, port: u16, size: usize, val: u32) {
        if let Some(entry) = self.ioports_for(port) {
            entry.io_out(port, size as usize, val);
            return;
        }
        self.debug_port(port, false);
    }

    fn debug_port(&mut self, port: u16, is_in: bool) {
        if self.last_unhandled_port != port {
            self.last_unhandled_port = port;
            let s = if is_in { "IN" } else { "OUT "};
            println!("unhandled io {} on port {:x}", s, port);
        }
    }

    pub fn emulate_mmio_write(&mut self, address: u64, size: usize, val: u64) {
        match self.mmio_for(address, size) {
            Some(d) => { d.write(address, size, val) },
            None => { println!("unhandled mmio write on address {:x}", address) }
        }
    }

    fn emulate_mmio_read(&mut self, address: u64, size: usize) -> u64 {
        match self.mmio_for(address, size) {
            Some(d) => { d.read(address, size) },
            None => { println!("unhandled mmio read on address {:x}", address); 0 }
        }
    }

    fn register_dummy(&mut self, port: u16, count: usize) {
        self.register_ioports(port, count, Arc::new(RwLock::new(IoPortDummy)));
    }

    fn setup_ioports(&mut self) {
        /* 0000 - 001F - DMA1 controller */
        self.register_dummy(0x0000, 32);
        /* 0020 - 003F - 8259A PIC 1 */
        self.register_dummy(0x0020, 2);
        /* 0060 - 0068 - i8042 */
        self.register_ioports(0x0060, 8, Arc::new(RwLock::new(IoPortFakeI8042(false))));
        /* 0040 - 005F - PIT (8253,8254) */
        self.register_dummy(0x0040, 4);
        /* 0092 - PS/2 system control port A */
        self.register_ioports(0x0092, 1, Arc::new(RwLock::new(IoPortPS2Control)));
        /* 00A0 - 00AF - 8259A PIC 1 */
        self.register_dummy(0x00A0, 2);
        /* 00C0 - 00CF - DMA1 controller */
        self.register_dummy(0x00C0, 32);
        /* 00F0 - 00FF - Math co-processor */
        self.register_dummy(0x00F0, 2);
        /* 0278 - 027A - Parallel printer port */
        self.register_dummy(0x0278, 3);
        /* 0378 - 037A - Parallel printer port */
        self.register_dummy(0x0378, 3);
        /* 03D4 - 03D5 - CRT Control registers */
        self.register_dummy(0x03D4, 2);
    }
}

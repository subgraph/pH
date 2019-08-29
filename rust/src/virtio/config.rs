use crate::memory::GuestRam;
use std::sync::Arc;

use crate::vm::Result;

use super::VirtQueue;
use super::eventfd::IoEventFd;
use super::vring::Vring;
use super::virtqueue::InterruptLine;
use super::bus::VirtioDeviceConfig;
use super::consts::DEFAULT_QUEUE_SIZE;

///
/// Manages a set of virtqueues during device intitialization.
///
pub struct VirtQueueConfig {
    num_queues: usize,
    selected_queue: u16,
    enabled_features: u64,
    vrings: Vec<Vring>,
    interrupt: Arc<InterruptLine>,
    events: Vec<Arc<IoEventFd>>,
}

impl VirtQueueConfig {
    pub fn new(memory: &GuestRam, dev_config: &VirtioDeviceConfig) -> Result<VirtQueueConfig> {
        Ok(VirtQueueConfig {
            num_queues: dev_config.num_queues(),
            selected_queue: 0,
            enabled_features: 0,
            vrings: create_vrings(memory,dev_config.num_queues()),
            interrupt: InterruptLine::from_config(&dev_config)?,
            events: create_ioeventfds(&dev_config)?,
        })
    }

    pub fn isr_read(&self) -> u64 {
        self.interrupt.isr_read()
    }

    pub fn notify_config(&self) {
        self.interrupt.notify_config();
    }


    pub fn enable_features(&mut self, features: u64) {
        self.enabled_features = features;
    }

    pub fn reset(&mut self) {
        self.selected_queue = 0;
        let _ = self.interrupt.isr_read();
        for vr in &mut self.vrings {
            vr.reset();
        }
    }

    pub fn num_queues(&self) -> u16 {
        self.num_queues as u16
    }

    pub fn selected_queue(&self) -> u16 {
        self.selected_queue
    }

    pub fn select_queue(&mut self, q: u16) {
        self.selected_queue = q;
    }

    pub fn with_vring<U,F>(&self, d: U, f: F) -> U
        where F: FnOnce(&Vring) -> U
    {
        match self.vrings.get(self.selected_queue as usize) {
            Some(vr) => f(vr),
            None => d,
        }
    }

    pub fn with_vring_mut<F>(&mut self, f: F)
        where F: FnOnce(&mut Vring)
    {
        match self.vrings.get_mut(self.selected_queue as usize) {
            Some(vr) => if !vr.is_enabled() { f(vr) },
            None => (),
        }
    }

    pub fn vring_get_size(&self) -> u16 { self.with_vring(0, |vr| vr.size() ) }
    pub fn vring_set_size(&mut self, sz: u16) { self.with_vring_mut(|vr| vr.set_size(sz)) }
    pub fn vring_enable(&mut self) { self.with_vring_mut(|vr| vr.enable() ) }
    pub fn vring_is_enabled(&self) -> bool { self.with_vring(false, |vr| vr.is_enabled() ) }

    pub fn notify(&self, vq: u16) {
        match self.events.get(vq as usize) {
            Some(ref ev) => ev.write(1).expect("ioeventfd write failed in notify"),
            None => (),
        }
    }

    fn create_vq(&self, memory: &GuestRam, idx: usize) -> Result<VirtQueue> {
        let vring = self.vrings[idx].clone();
        vring.validate()?;
        Ok(VirtQueue::new(memory.clone(), vring, self.interrupt.clone(), self.events[idx].clone()))
    }

    pub fn create_queues(&self, memory: &GuestRam) -> Result<Vec<VirtQueue>> {
        let mut v = Vec::with_capacity(self.num_queues);
        for i in 0..self.num_queues {
            v.push(self.create_vq(memory, i)?);
        }
        Ok(v)
    }
}

fn create_ioeventfds(conf: &VirtioDeviceConfig) -> Result<Vec<Arc<IoEventFd>>> {
    let mut v = Vec::with_capacity(conf.num_queues());
    let notify_base = conf.notify_mmio().base();

    for i in 0..conf.num_queues() {
        let evt = IoEventFd::new(conf.kvm(), notify_base + (4 * i as u64))?;
        v.push(Arc::new(evt));
    }
    Ok(v)
}

fn create_vrings(memory: &GuestRam, n: usize) -> Vec<Vring> {
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(Vring::new(memory.clone(), DEFAULT_QUEUE_SIZE));
    }
    v
}

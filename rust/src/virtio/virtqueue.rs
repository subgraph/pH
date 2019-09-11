use std::sync::atomic::{Ordering, AtomicUsize, AtomicBool};
use std::sync::Arc;
use std::os::unix::io::AsRawFd;

use crate::memory::GuestRam;
use crate::kvm::Kvm;
use crate::vm::Result;

use super::eventfd::{EventFd,IoEventFd};
use super::consts::*;
use super::vring::{Vring,Descriptor};
use super::bus::VirtioDeviceConfig;
use super::chain::Chain;

#[derive(Clone)]
pub struct VirtQueue {
    memory: GuestRam,
    vring: Vring,
    features: u64,
    ioeventfd: Arc<IoEventFd>,
    interrupt: Arc<InterruptLine>,
    closed: Arc<AtomicBool>,
}

impl VirtQueue {
    pub fn new(memory: GuestRam, vring: Vring, interrupt: Arc<InterruptLine>, ioeventfd: Arc<IoEventFd>) -> VirtQueue {
        VirtQueue {
            memory,
            vring,
            features: 0,
            ioeventfd,
            interrupt,
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    #[allow(dead_code)]
    pub fn set_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.ioeventfd.write(1).unwrap();
    }

    #[allow(dead_code)]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn use_event_idx(&self) -> bool {
        self.features & VIRTIO_F_EVENT_IDX != 0
    }

    pub fn wait_ready(&self) -> Result<()> {
        if self.vring.is_empty() {
            let _ = self.ioeventfd.read()?;
        }
        Ok(())
    }

    pub fn wait_next_chain(&self) -> Result<Chain> {
        loop {
            self.wait_ready()?;
            if let Some(idx) = self.pop_avail_entry() {
                return Ok(Chain::new(self.memory.clone(), self.clone(), idx, self.vring.size()));
            }
        }
    }

    pub fn next_chain(&self) -> Option<Chain> {
        self.pop_avail_entry()
            .map(|idx| Chain::new(self.memory.clone(), self.clone(), idx, self.vring.size()))
    }

    pub fn on_each_chain<F>(&self, mut f: F)
        where F: FnMut(Chain) {
        loop {
            self.wait_ready().unwrap();
            for chain in self.iter() {
                f(chain);
            }
        }
    }

    pub fn iter(&self) -> QueueIter {
        QueueIter { vq: self.clone() }
    }

    fn need_interrupt(&self, first_used: u16, used_count: usize) -> bool {
        if used_count == 0 {
            return false;
        }
        if self.use_event_idx() {
            let event = self.vring.read_used_event();
            // Minimum count needed to traverse event idx
            let span = ((event - first_used) + 1) as usize;
            return used_count >= span;
        }
        !self.vring.read_avail_no_interrupt()
    }

    pub fn put_used(&self, idx: u16, len: u32) {
        let used = self.vring.next_used();
        self.vring.put_used(idx, len);
        if self.need_interrupt(used, 1) {
            self.interrupt.notify_queue();
        }
    }

    fn pop_avail_entry(&self) -> Option<u16> {
        if let Some(idx) = self.vring.pop_avail_entry() {
            if self.use_event_idx() {
                self.vring.write_avail_event(self.vring.next_avail());
            }
            return Some(idx)
        }
        None
    }

    pub fn load_descriptor(&self, idx: u16) -> Option<Descriptor> {
        self.vring.load_descriptor(idx)
    }

    pub fn ioevent(&self) -> &IoEventFd {
        &self.ioeventfd
    }
}

pub struct QueueIter {
    vq: VirtQueue,
}

impl Iterator for QueueIter {
    type Item =  Chain;

    fn next(&mut self) -> Option<Self::Item> {
        self.vq.pop_avail_entry().map(|idx| {
            Chain::new(self.vq.memory.clone(),self.vq.clone(),idx, self.vq.vring.size())
        })
    }
}


pub struct InterruptLine {
    irqfd: EventFd,
    isr: AtomicUsize,
}

impl InterruptLine {
    pub fn from_config(conf: &VirtioDeviceConfig) -> Result<Arc<InterruptLine>> {
        InterruptLine::new(conf.kvm(), conf.irq())
    }

    fn new(kvm: &Kvm, irq: u8) -> Result<Arc<InterruptLine>> {
        let irqfd = EventFd::new()?;
        kvm.irqfd(irqfd.as_raw_fd() as u32, irq as u32)?;
        Ok(Arc::new(InterruptLine{
            irqfd,
            isr: AtomicUsize::new(0)
        }))
    }

    pub fn isr_read(&self) -> u64 {
        self.isr.swap(0, Ordering::SeqCst) as u64
    }

    pub fn notify_queue(&self) {
        self.isr.fetch_or(0x1, Ordering::SeqCst);
        self.irqfd.write(1).unwrap();
    }

    pub fn notify_config(&self) {
        self.isr.fetch_or(0x2, Ordering::SeqCst);
        self.irqfd.write(1).unwrap();
    }
}



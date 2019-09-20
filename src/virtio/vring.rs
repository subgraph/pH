use std::sync::atomic::{self,AtomicUsize,Ordering};
use std::sync::Arc;
use std::fmt;
use std::cmp;
use std::io::{self, Read};

use crate::memory::GuestRam;
use super::consts::*;

use crate::vm::{Result,Error,ErrorKind};

///
/// A convenience wrapper around `AtomicUsize`
///
#[derive(Clone)]
struct SharedIndex(Arc<AtomicUsize>);

impl SharedIndex {
    fn new() -> SharedIndex {
        SharedIndex(Arc::new(AtomicUsize::new(0)))
    }
    fn get(&self) -> u16 {
        self.0.load(Ordering::SeqCst) as u16
    }
    fn inc(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn set(&self, v: u16) {
        self.0.store(v as usize, Ordering::SeqCst);
    }
}

///
/// Access to the low-level memory structure of a Virtqueue.
///
#[derive(Clone)]
pub struct Vring {
    memory: GuestRam,
    /// Default queue_size for this virtqueue
    default_size: u16,
    /// Number of elements in the virtqueue ring
    queue_size: u16,
    /// Guest address for base of descriptor table
    pub descriptors: u64,
    /// Guest address for base of available ring
    pub avail_ring: u64,
    /// Guest address for base of used ring
    pub used_ring: u64,

    /// Has this virtqueue been enabled?
    enabled: bool,

    /// The index in the used ring where the next used entry will be placed
    next_used_idx: SharedIndex,
    /// last seen avail_idx loaded from guest memory
    cached_avail_idx: SharedIndex,
    /// The index in the avail ring where the next available entry will be read
    next_avail: SharedIndex,
}

impl Vring {

    pub fn new(memory: GuestRam, default_size: u16) -> Vring {
        Vring {
            memory,
            default_size,
            queue_size: default_size,
            descriptors:0,
            avail_ring: 0,
            used_ring: 0,
            enabled: false,

            next_used_idx: SharedIndex::new(),
            cached_avail_idx: SharedIndex::new(),
            next_avail: SharedIndex::new(),
        }

    }

    ///
    /// Set `Vring` into the enabled state.
    ///
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    ///
    /// Return `true` if this `Vring` has been enabled.
    ///
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    ///
    /// Queue size of this `Vring`
    ///
    pub fn size(&self) -> u16 {
        self.queue_size
    }

    ///
    /// Set the queue size of this `Vring`.  If `sz` is an invalid value
    /// ignore the request.  It is illegal to change the queue size after
    /// a virtqueue has been enabled, so ignore requests if enabled.
    ///
    /// Valid sizes are less than or equal to `MAX_QUEUE_SIZE` and must
    /// be a power of 2.
    ///
    pub fn set_size(&mut self, sz: u16) {
        if self.enabled || sz > MAX_QUEUE_SIZE || (sz & (sz - 1) != 0) {
            return;
        }
        self.queue_size = sz;
    }

    ///
    /// Reset `Vring` to the initial state.  `queue_size` is set to the `default_size`
    /// and all other fields are cleared.  `enabled` is set to false.
    ///
    pub fn reset(&mut self) {
        self.queue_size = self.default_size;
        self.descriptors = 0;
        self.avail_ring = 0;
        self.used_ring = 0;
        self.enabled = false;
        self.next_used_idx.set(0);
        self.cached_avail_idx.set(0);
        self.next_avail.set(0);
    }


    ///
    /// Does `Vring` currently have available entries?
    ///
    /// Queue is empty if `next_avail` is same value as
    /// `avail_ring.idx` value in guest memory If `cached_avail_idx`
    /// currently matches `next_avail` it is reloaded from
    /// memory in case guest has updated field since last
    /// time it was loaded.
    ///
    pub fn is_empty(&self) -> bool {
        let next_avail = self.next_avail.get();
        if self.cached_avail_idx.get() != next_avail {
            return false;
        }
        next_avail == self.load_avail_idx()
    }

    ///
    /// Write an entry into the Used ring.
    ///
    /// The entry is written into the ring structure at offset
    /// `next_used_idx % queue_size`.  The value of `next_used_idx`
    /// is then incremented and the new value is written into
    /// guest memory into the `used_ring.idx` field.
    ///
    pub fn put_used(&self, idx: u16, len: u32) {
        if idx >= self.queue_size {
            return;
        }

        let used_idx = (self.next_used_idx.get() % self.queue_size) as u64;
        let elem_addr = self.used_ring + (4 + used_idx * 8);
        // write descriptor index to 'next used' slot in used ring
        self.memory.write_int(elem_addr, idx as u32).unwrap();
        // write length to 'next used' slot in ring
        self.memory.write_int(elem_addr + 4, len as u32).unwrap();

        self.next_used_idx.inc();
        atomic::fence(Ordering::Release);
        // write updated next_used
        self.memory.write_int(self.used_ring + 2, self.next_used_idx.get()).unwrap();
    }


    ///
    /// Load `avail_ring.idx` from guest memory and store it in `cached_avail_idx`.
    ///
    pub fn load_avail_idx(&self) -> u16 {
        let avail_idx = self.memory.read_int::<u16>(self.avail_ring + 2).unwrap();
        self.cached_avail_idx.set(avail_idx);
        avail_idx
    }

    ///
    /// Read from guest memory and return the Avail ring entry at
    /// index `ring_idx % queue_size`.
    ///
    fn load_avail_entry(&self, ring_idx: u16) -> u16 {
        let offset = (4 + (ring_idx % self.queue_size) * 2) as u64;
        self.memory.read_int(self.avail_ring + offset).unwrap()
    }

    ///
    /// If queue is not empty, read and return the next Avail ring entry
    /// and increment `next_avail`.  If queue is empty return `None`
    ///
    pub fn pop_avail_entry(&self) -> Option<u16> {
        if self.is_empty() {
            return None
        }
        let next_avail = self.next_avail.get();
        let avail_entry = self.load_avail_entry(next_avail);
        self.next_avail.inc();
        Some(avail_entry)
    }

    pub fn next_avail(&self) -> u16 {
        self.next_avail.get() % self.queue_size
    }

    ///
    /// Read and return the `used_event` field from the Avail ring.
    ///
    pub fn read_used_event(&self) -> u16 {
        let addr = self.avail_ring + 4 + (self.queue_size as u64 * 2);
        self.memory.read_int::<u16>(addr).unwrap()
    }

    ///
    /// Read the `flags` field from the Avail ring and return `true` if
    /// `NO_INTERRUPT` bit is set.
    ///
    pub fn read_avail_no_interrupt(&self) -> bool {
        let flags = self.memory.read_int::<u16>(self.avail_ring).unwrap();
        flags & 0x01 != 0
    }

    ///
    /// Write `val` to the `avail_event` field of Used ring.
    ///
    /// If `val` is not a valid index for this virtqueue this
    /// function does nothing.
    ///
    pub fn write_avail_event(&self, val: u16) {
        if val > self.queue_size {
            return;
        }
        let addr = self.used_ring + 4 + (self.queue_size as u64 * 8);
        self.memory.write_int::<u16>(addr, val).unwrap();
        atomic::fence(Ordering::Release);
    }

    ///
    /// Set or clear the `NO_NOTIFY` bit in flags field of Used ring
    ///
    #[allow(dead_code)]
    pub fn write_used_no_notify(&self, val: bool) {
        let flag = if val { 0x1 } else { 0x0 };
        self.memory.write_int::<u16>(self.used_ring, flag).unwrap();
    }

    ///
    /// Load the descriptor table entry at `idx` from guest memory and return it.
    ///
    pub fn load_descriptor(&self, idx: u16) -> Option<Descriptor> {
        if idx >= self.queue_size {
            panic!("load_descriptor called with index larger than queue size");
        }
        let head = self.descriptors + (idx as u64 * 16);

        let addr = self.memory.read_int::<u64>(head).unwrap();
        let len= self.memory.read_int::<u32>(head + 8).unwrap();
        let flags = self.memory.read_int::<u16>(head + 12).unwrap();
        let next = self.memory.read_int::<u16>(head + 14).unwrap();

        if self.memory.is_valid_range(addr, len as usize) && next < self.queue_size {
            return Some(Descriptor::new(idx, addr, len, flags, next));
        }
        None
    }

    pub fn next_used(&self) -> u16 {
        self.next_used_idx.get()
    }

    pub fn validate(&self) -> Result<()> {
        fn vring_err<T: ToString>(msg: T) -> Result<()> {
            Err(Error::new(ErrorKind::InvalidVring, msg.to_string()))
        }

        if !self.enabled {
            return vring_err("vring is not enabled");
        }
        let qsz = self.queue_size as usize;
        let desc_table_sz = 16 * qsz;
        let avail_ring_sz = 6 + 2 * qsz;
        let used_ring_sz = 6 + 8 * qsz;
        if !self.memory.is_valid_range(self.descriptors, desc_table_sz) {
            return vring_err(format!("descriptor table range is invalid 0x{:x}", self.descriptors));
        }
        if !self.memory.is_valid_range(self.avail_ring, avail_ring_sz) {
            return vring_err(format!("avail ring range is invalid 0x{:x}", self.avail_ring));
        }
        if !self.memory.is_valid_range(self.used_ring, used_ring_sz) {
            return vring_err(format!("used ring range is invalid 0x{:x}", self.used_ring));
        }
        Ok(())
    }
}

///
/// An entry read from the descriptor table
///
#[derive(Copy,Clone)]
pub struct Descriptor {
    pub idx: u16,
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

impl Descriptor {
    fn new(idx: u16, addr: u64, len: u32, flags: u16, next:u16) -> Descriptor {
        Descriptor{ idx, addr, len, flags, next }
    }

    ///
    /// Test if `flag` is set in `self.flags`
    ///
    fn has_flag(&self, flag: u16) -> bool {
        self.flags & flag == flag
    }

    ///
    /// Is VRING_DESC_F_NEXT set in `self.flags`?
    ///
    pub fn has_next(&self) -> bool {
        self.has_flag(VRING_DESC_F_NEXT)
    }

    ///
    /// Is VRING_DESC_F_WRITE set in `self.flags`?
    ///
    pub fn is_write(&self) -> bool {
        self.has_flag(VRING_DESC_F_WRITE)
    }

    ///
    /// Is VRING_DESC_F_INDIRECT set in `self.flags`?
    ///
    #[allow(dead_code)]
    pub fn is_indirect(&self) -> bool {
        self.has_flag(VRING_DESC_F_INDIRECT)
    }

    pub fn remaining(&self, offset: usize) -> usize {
        if offset >= self.len as usize {
            0
        } else {
            self.len as usize - offset
        }
    }

    pub fn read_from(&self, memory: &GuestRam, offset: usize, buf: &mut[u8]) -> usize {
        let sz = cmp::min(buf.len(), self.remaining(offset));
        if sz > 0 {
            memory.read_bytes(self.addr + offset as u64, buf).unwrap();
        }
        sz
    }

    pub fn write_to(&self, memory: &GuestRam, offset: usize, buf: &[u8]) -> usize {
        let sz = cmp::min(buf.len(), self.remaining(offset));
        if sz > 0 {
            memory.write_bytes(self.addr + offset as u64, buf).unwrap();
        }
        sz
    }

    pub fn write_from_reader<R: Read+Sized>(&self, memory: &GuestRam, offset: usize, mut r: R, size: usize) -> io::Result<usize> {
        let sz = cmp::min(size, self.remaining(offset));
        if sz > 0 {
            let slice = memory.mut_slice(self.addr + offset as u64, sz).unwrap();
            return r.read(slice);
        }
        Ok(0)
    }
}

impl fmt::Debug for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Descriptor{{ idx: {} addr: {:x} len: {} flags: {:x} next: {} }}", self.idx, self.addr, self.len, self.flags, self.next)
    }
}



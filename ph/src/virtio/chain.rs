
use std::io::{self,Read,Write};

use memory::GuestRam;
use super::VirtQueue;
use super::vring::Descriptor;

pub struct Chain {

    memory: GuestRam,

    vq: VirtQueue,

    /// Number of remaining descriptors allowed in this chain.
    ttl: u16,

    /// Current descriptor or `None` if at end of chain
    current: Option<Descriptor>,

    /// Offset for read/write into current descriptor
    offset: usize,

    /// Saved head index to place in used ring.  Set to `None`
    /// after writing to used ring.
    head_idx: Option<u16>,

    /// Number of bytes written into writeable descriptors
    /// in this chain. Will be written into used ring later.
    wlen: usize,
}


impl Chain {
    pub fn new(memory: GuestRam, vq: VirtQueue, head: u16, ttl: u16) -> Chain {
        let first = vq.load_descriptor(head);
        Chain {
            memory,
            vq, ttl, head_idx: Some(head),
            current: first,
            offset: 0, wlen: 0,
        }
    }

    /// Applies a function to the current descriptor (if `Some`) or
    /// returns default parameter `d` (if `None`).
    pub fn with_current_descriptor<U,F>(&self, d: U, f: F) -> U
        where F: FnOnce(&Descriptor) -> U {
        match self.current {
            Some(ref desc) => f(desc),
            None => d,
        }
    }

    /// Load and return next descriptor from chain.
    ///
    /// If `self.current`
    ///
    ///   1) holds a descriptor (`self.current.is_some()`)
    ///   2) that descriptor has a next field (`desc.has_next()`)
    ///   3) time-to-live is not zero (`self.ttl > 0`)
    ///
    /// then load and return the descriptor pointed to by the current
    /// descriptor. Returns `None` otherwise.
    ///
    fn next_desc(&self) -> Option<Descriptor> {
        self.with_current_descriptor(None, |desc| {
            if desc.has_next() && self.ttl > 0 {
               self.vq.load_descriptor(desc.next)
            } else {
                None
            }
        })
    }

    /// Load next descriptor in chain into `self.current`.
    ///
    /// Set `self.current` to the next descriptor in chain or `None` if
    /// at end of chain.
    ///
    pub fn load_next_descriptor(&mut self) {
        self.current = self.next_desc();
        // Only decrement ttl if a new descriptor was loaded
        if self.current.is_some() {
            self.ttl -= 1;
        }
        self.offset = 0;
    }

    ///
    /// Return `true` if current descriptor exists and is readable, otherwise
    /// `false`.
    ///
    pub fn is_current_readable(&self) -> bool {
        self.with_current_descriptor(false, |desc| !desc.is_write())
    }

    ///
    /// If `current` is a writeable descriptor, keep loading new descriptors until
    /// a readable descriptor is found or end of chain is reached.  After this
    /// call `current` will either be a readable descriptor or `None` if the
    /// end of chain was reached.
    ///
    pub fn skip_readable(&mut self) {
        while self.is_current_readable() {
            self.load_next_descriptor();
        }
    }

    /// Return `true` if the end of the descriptor chain has been reached.
    ///
    /// When at end of chain `self.current` is `None`.
    pub fn is_end_of_chain(&self) -> bool {
        self.current.is_none()
    }

    ///
    /// Length field of current descriptor is returned or 0 if
    /// at end of chain.
    ///
    fn current_size(&self) -> usize {
        self.with_current_descriptor(0, |desc| desc.len as usize)
    }

    ///
    /// Increment `self.offset` with the number of bytes
    /// read or written from `current` descriptor and
    /// load next descriptor if `current` descriptor
    /// has been fully consumed.
    ///
    fn inc_offset(&mut self, sz: usize) {
        self.offset += sz;
        if self.offset >= self.current_size() {
            self.load_next_descriptor();
        }
    }

    ///
    /// Read from the `current` readable descriptor and return
    /// the number of bytes read.
    ///
    /// If this read exhausts the `current` descriptor then the
    /// next descriptor in chain will be loaded into `current`.
    ///
    /// Assumes that current is a readable descriptor so caller must
    /// call `self.is_current_readable()` before calling this.
    ///
    fn read_current(&mut self, bytes: &mut[u8]) -> usize {
        assert!(self.is_current_readable());

        let nread = self.with_current_descriptor(0, |desc| {
            desc.read_from(&self.memory, self.offset, bytes)
        });
        self.inc_offset(nread);
        nread
    }

    ///
    /// Write into the `current` writeable descriptor if it exists
    /// and return the number of bytes read or 0 if at end of chain.
    ///
    /// If this write exausts the `current` descriptor then the
    /// next descriptor in chain will be loaded into `current`
    ///
    /// Assumes that `current` is a writeable descriptor or `None`
    /// so caller must call `self.skip_readable()` before calling this.
    ///
    fn write_current(&mut self, bytes: &[u8]) -> usize {
        assert!(!self.is_current_readable());
        let sz = self.with_current_descriptor(0, |desc| {
            desc.write_to(&self.memory, self.offset, bytes)
        });
        self.inc_offset(sz);
        sz
    }

    ///
    /// Write this chain head index (`self.head_idx`) and bytes written (`self.wlen`)
    /// into used ring. Consumes `self.head_idx` so that used ring cannot
    /// accidentally be written more than once.  Since we have returned this
    /// chain to the guest, it is no longer valid to access any descriptors in
    /// this chain so `self.current` is set to `None`.
    ///
    pub fn flush_chain(&mut self) {
        match self.head_idx {
            Some(idx) => self.vq.put_used(idx, self.wlen as u32),
            None => (),
        }
        self.current = None;
        self.head_idx = None;
    }

    pub fn current_write_address(&mut self, size: usize) -> Option<u64> {
        self.skip_readable();
        self.with_current_descriptor(None, |desc| {
            if desc.len as usize - self.offset < size {
                None
            } else {
                Some(desc.addr + self.offset as u64)
            }
        })
    }

    pub fn get_wlen(&self) -> usize {
        self.wlen
    }

    #[allow(dead_code)]
    pub fn debug(&self) {
        self.with_current_descriptor((), |desc| {
            println!("offset: {} desc: {:?}", self.offset, desc);
        });
    }

    pub fn copy_from_reader<R: Read+Sized>(&mut self, r: R, size: usize) -> io::Result<usize> {
        self.skip_readable();
        assert!(!self.is_current_readable());

        let res = self.with_current_descriptor(Ok(0usize), |desc| {
            desc.write_from_reader(&self.memory, self.offset,r, size)
        });
        if let Ok(nread) = res {
            self.inc_offset(nread);
            self.wlen += nread;
        }
        res
    }

    /*
    pub fn copy_to_writer<W: Write+Sized>(&mut self, w: W, size: usize) -> io::Result<usize> {
        unimplemented!()

    }
    */

}

impl Drop for Chain {
    fn drop(&mut self) {
        self.flush_chain();
    }
}

impl Read for Chain {
    // nb: does not fail, but can read short
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut nread = 0usize;
        while self.is_current_readable() && nread < buf.len() {
            nread += self.read_current(&mut buf[nread..]);
        }
        Ok(nread)
    }
}

impl Write for Chain {
    // nb: does not fail, but can write short
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.skip_readable();
        let mut nwrote = 0usize;
        while !self.is_end_of_chain() && nwrote < buf.len() {
            nwrote += self.write_current(&buf[nwrote..]);
        }
        self.wlen += nwrote;
        Ok(nwrote)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

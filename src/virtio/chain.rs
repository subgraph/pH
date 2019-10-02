use std::fmt;
use std::io::{self,Read,Write};

use crate::memory::GuestRam;
use crate::virtio::VirtQueue;
use crate::virtio::vring::Descriptor;

struct DescriptorList {
    memory: GuestRam,
    descriptors: Vec<Descriptor>,
    offset: usize,
    total_size: usize,
    consumed_size: usize,
}

impl DescriptorList {
    fn new(memory: GuestRam) -> Self {
        DescriptorList {
            memory,
            descriptors: Vec::new(),
            offset: 0,
            total_size: 0,
            consumed_size: 0,
        }
    }

    fn add_descriptor(&mut self, d: Descriptor) {
        self.total_size += d.len as usize;
        self.descriptors.push(d)
    }

    fn reverse(&mut self) {
        self.descriptors.reverse();
    }

    fn clear(&mut self) {
        self.descriptors.clear();
        self.offset = 0;
    }

    fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }

    fn current(&self) -> Option<&Descriptor> {
        self.descriptors.last()
    }

    fn current_address(&self, size: usize) -> Option<u64> {
        self.current().and_then(|d| {
            if d.remaining(self.offset) >= size {
                Some(d.addr + self.offset as u64)
            } else {
                None
            }
        })
    }

    fn inc(&mut self, len: usize) {
        let d = match self.current() {
            Some(d) => d,
            None => {
                warn!("Virtqueue increment called with no current descriptor");
                return;
            }
        };
        let remaining = d.remaining(self.offset);
        if len > remaining {
            warn!("Virtqueue descriptor buffer increment exceeds current size");
        }
        if len >= remaining {
            self.consumed_size += remaining;
            self.offset = 0;
            self.descriptors.pop();
        } else {
            self.consumed_size += len;
            self.offset += len;
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> usize {
        if let Some(d) = self.current() {
            let n = d.read_from(&self.memory, self.offset, buf);
            self.inc(n);
            return n;
        }
        0
    }

    fn write(&mut self, buf: &[u8]) -> usize {
        if let Some(d) = self.current() {
            let n = d.write_to(&self.memory, self.offset, buf);
            self.inc(n);
            return n;
        }
        0
    }

    fn write_from_reader<R>(&mut self, reader: R, size: usize) -> io::Result<usize>
        where R: Read+Sized
    {
        if let Some(d) = self.current() {
            let n = d.write_from_reader(&self.memory, self.offset, reader, size)?;
            self.inc(n);
            Ok(n)
        } else {
            Ok(0)
        }
    }

    fn current_slice(&self) -> &[u8] {
        if let Some(d) = self.current() {
            let size = d.remaining(self.offset);
            let addr = d.addr + self.offset as u64;
            self.memory.slice(addr, size).unwrap_or(&[])
        } else {
            &[]
        }
    }

    fn current_mut_slice(&self) -> &mut [u8] {
        if let Some(d) = self.current() {
            let size = d.remaining(self.offset);
            let addr = d.addr + self.offset as u64;
            self.memory.mut_slice(addr, size).unwrap_or(&mut [])
        } else {
            &mut []
        }
    }

    fn remaining(&self) -> usize {
        self.total_size - self.consumed_size
    }
}

impl fmt::Debug for DescriptorList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DList[size={}, [", self.total_size)?;
        for d in self.descriptors.iter().rev() {
            write!(f, "(#{}, 0x{:08x}, [{}]),", d.idx, d.addr, d.len)?;
        }
        write!(f, "]")
    }
}

pub struct Chain {
    head: Option<u16>,
    vq: VirtQueue,
    readable: DescriptorList,
    writeable: DescriptorList,
}

impl Chain {
    pub fn new(memory: GuestRam, vq: VirtQueue, head: u16, ttl: u16) -> Self {
        let (readable,writeable) = Self::load_descriptors(memory, &vq, head, ttl);
        Chain {
            head: Some(head),
            vq,
            readable,
            writeable,
        }
    }

    fn load_descriptors(memory: GuestRam, vq: &VirtQueue, head: u16, ttl: u16) -> (DescriptorList, DescriptorList) {
        let mut readable = DescriptorList::new(memory.clone());
        let mut writeable = DescriptorList::new(memory);
        let mut idx = head;
        let mut ttl = ttl;

        while let Some(d) = vq.load_descriptor(idx) {
            if ttl == 0 {
                warn!("Descriptor chain length exceeded ttl");
                break;
            } else {
                ttl -= 1;
            }

            if d.is_write() {
                writeable.add_descriptor(d);
            } else {
                if !writeable.is_empty() {
                    warn!("Guest sent readable virtqueue descriptor after writeable descriptor in violation of specification");
                }
                readable.add_descriptor(d);
            }
            if !d.has_next() {
                break;
            }
            idx = d.next;
        }
        readable.reverse();
        writeable.reverse();
        return (readable, writeable);
    }

    pub fn w8(&mut self, n: u8) -> io::Result<()> {
        self.write_all(&[n])?;
        Ok(())
    }
    pub fn w16(&mut self, n: u16) -> io::Result<()> {
        self.write_all(&n.to_le_bytes())?;
        Ok(())
    }
    pub fn w32(&mut self, n: u32) -> io::Result<()> {
        self.write_all(&n.to_le_bytes())?;
        Ok(())
    }
    pub fn w64(&mut self, n: u64) -> io::Result<()> {
        self.write_all(&n.to_le_bytes())?;
        Ok(())
    }

    pub fn r16(&mut self) -> io::Result<u16> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }
    pub fn r32(&mut self) -> io::Result<u32> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    pub fn r64(&mut self) -> io::Result<u64> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    pub fn flush_chain(&mut self) {
        if let Some(head) = self.head.take() {
            self.readable.clear();
            self.writeable.clear();
            self.vq.put_used(head, self.writeable.consumed_size as u32);
        }
    }

    pub fn current_write_address(&mut self, size: usize) -> Option<u64> {
        self.writeable.current_address(size)
    }

    pub fn remaining_read(&self) -> usize {
        self.readable.remaining()
    }

    pub fn remaining_write(&self) -> usize {
        self.writeable.remaining()
    }

    pub fn get_wlen(&self) -> usize {
        self.writeable.consumed_size
    }

    pub fn is_end_of_chain(&self) -> bool {
        self.readable.is_empty() && self.writeable.is_empty()
    }

    pub fn current_read_slice(&self) -> &[u8] {
        self.readable.current_slice()
    }

    pub fn inc_read_offset(&mut self, sz: usize) {
        self.readable.inc(sz);
    }

    pub fn inc_write_offset(&mut self, sz: usize) {
        if !self.readable.is_empty() {
            self.readable.clear();
        }
        self.writeable.inc(sz);
    }

    pub fn current_write_slice(&mut self) -> &mut [u8] {
        self.writeable.current_mut_slice()
    }

    pub fn copy_from_reader<R>(&mut self, r: R, size: usize) -> io::Result<usize>
    where R: Read+Sized
    {
        self.writeable.write_from_reader(r, size)
    }
}

impl Read for Chain {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut nread = 0usize;
        while nread < buf.len() {
            nread += match self.readable.read(&mut buf[nread..]) {
                0 => return Ok(nread),
                n => n,
            };
        }
        Ok(nread)
    }
}
impl Write for Chain {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut nwrote = 0;
        while nwrote < buf.len() {
            match self.writeable.write(&buf[nwrote..]) {
                0 => return Ok(nwrote),
                n => nwrote += n,
            };
        }
        Ok(nwrote)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.flush_chain();
    }
}

impl fmt::Debug for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Chain {{ R {:?} W {:?} }}", self.readable, self.writeable)
    }
}

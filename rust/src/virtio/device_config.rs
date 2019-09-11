
use byteorder::{ByteOrder,LittleEndian};
use std::ops::Range;

pub struct DeviceConfigArea {
    buffer: Vec<u8>,
    write_filter: DeviceConfigWriteFilter,
}


#[allow(dead_code)]
impl DeviceConfigArea {
    pub fn new(size: usize) -> Self {
        DeviceConfigArea{
            buffer: vec![0u8; size],
            write_filter: DeviceConfigWriteFilter::new(size),
        }
    }

    pub fn read_config(&self, offset: usize, size: usize) -> u64 {
        if offset + size > self.buffer.len() {
            return 0;
        }
        match size {
            1 => self.buffer[offset] as u64,
            2 => LittleEndian::read_u16(&self.buffer[offset..]) as u64,
            4 => LittleEndian::read_u32(&self.buffer[offset..]) as u64,
            8 => LittleEndian::read_u64(&self.buffer[offset..]),
            _ => 0,
        }
    }

    pub fn write_config(&mut self, offset: usize, size: usize, val: u64) {
        if self.write_filter.is_writeable(offset, size) {
            match size {
                1 => self.write_u8(offset, val as u8),
                2 => self.write_u16(offset, val as u16),
                4 => self.write_u32(offset, val as u32),
                8 => self.write_u64(offset, val as u64),
                _ => {},
            }
        }
    }

    pub fn set_writeable(&mut self, offset: usize, size: usize) {
        self.write_filter.set_writable(offset, size)
    }

    pub fn write_u8(&mut self, offset: usize, val: u8) {
        assert!(offset + 1 <= self.buffer.len());
        self.buffer[offset] = val;
    }

    pub fn write_u16(&mut self, offset: usize, val: u16) {
        assert!(offset + 2 <= self.buffer.len());
        LittleEndian::write_u16(&mut self.buffer[offset..], val);
    }

    pub fn write_u32(&mut self, offset: usize, val: u32) {
        assert!(offset + 4 <= self.buffer.len());
        LittleEndian::write_u32(&mut self.buffer[offset..], val);
    }

    pub fn write_u64(&mut self, offset: usize, val: u64) {
        assert!(offset + 8 <= self.buffer.len());
        LittleEndian::write_u64(&mut self.buffer[offset..], val);
    }

    pub fn write_bytes(&mut self, offset: usize, bytes: &[u8]) {
        assert!(offset + bytes.len() <= self.buffer.len());
        self.buffer[offset..offset + bytes.len()].copy_from_slice(bytes);
    }
}

struct DeviceConfigWriteFilter {
    size: usize,
    ranges: Vec<Range<usize>>,
}

impl DeviceConfigWriteFilter {
    fn new(size: usize) -> Self {
        DeviceConfigWriteFilter { size, ranges: Vec::new() }
    }

    fn set_writable(&mut self, offset: usize, size: usize) {
        let end = offset + size;
        self.ranges.push(offset..end);
    }

    fn is_writeable(&self, offset: usize, size: usize) -> bool {
        if offset + size > self.size {
            false
        } else {
            let last = offset + size - 1;
            self.ranges.iter().any(|r| r.contains(&offset) && r.contains(&last))
        }
    }
}
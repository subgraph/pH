use std::fmt;

#[derive(Copy,Clone,Debug,Ord,PartialOrd,Eq,PartialEq)]
pub struct AddressRange {
    start: u64, // inclusive
    end: u64,   // exclusive
}

impl fmt::Display for AddressRange {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "AddressRange(0x{:x} - 0x{:x}) [size: {}]", self.start, self.end - 1, self.size())
    }
}

impl AddressRange {
    pub fn checked_new(base: u64, size: usize) -> Option<AddressRange> {
        match base.checked_add(size as u64) {
            Some(end) if size > 0 => Some(AddressRange{ start:base, end }),
            _ => None,
        }
    }

    pub fn new(base: u64, size: usize) -> AddressRange {
        assert!(size > 0, "cannot construct address range with size = 0");
        AddressRange::checked_new(base, size)
            .expect(format!("Address range overflows base: {:x} size: {}", base, size).as_str())
    }

    pub fn contains_address(&self, address: u64) -> bool {
        address >= self.start && address < self.end
    }

    pub fn contains(&self, address: u64, size: usize) -> bool {
        assert!(size > 0, "size cannot be 0, use contains_address() for single address test");
        match address.checked_add(size as u64) {
            Some(end) => self.contains_address(address) && self.contains_address(end - 1),
            None => false,
        }
    }

    pub fn checked_offset_of(&self, address: u64) -> Option<usize> {
        if self.contains_address(address) {
            Some((address - self.start) as usize)
        } else {
            None
        }
    }

    pub fn offset_of(&self, address: u64) -> usize {
        self.checked_offset_of(address).expect("range does not contain address for call to offset_into()")
    }

    pub fn checked_offset_into(&self, offset: usize) -> Option<u64> {
        match self.start.checked_add(offset as u64) {
            Some(addr) if self.contains_address(addr) => Some(addr),
            _ => None,
        }
    }

    pub fn subrange(&self, offset: usize, size: usize) -> Option<AddressRange> {
        match self.checked_offset_into(offset) {
            Some(base) if self.contains(base, size) => Some(AddressRange::new(base, size)),
            _ => None,
        }
    }

    pub fn base(&self) -> u64 { self.start }

    pub fn end(&self) -> u64 { self.end }

    pub fn size(&self) -> usize { (self.end - self.start) as usize }

    pub fn is_base2_sized(&self) -> bool {
        let sz = self.size();
        sz & (sz - 1) == 0
    }

    pub fn is_naturally_aligned(&self) -> bool {
        self.is_base2_sized() && (self.base() % (self.size() as u64) == 0)
    }
}

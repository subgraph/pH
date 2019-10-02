/// An efficiently stored array (or set) of bits.
///
/// Bits can be set, cleared, or tested by index into the
/// array of bits. Since the methods are named to follow
/// the set collection convention you can also think of
/// it as a set which stores `usize` index values.
///
pub struct BitSet {
    blocks: Vec<u64>,
}

impl BitSet {

    /// Create a new empty `BitSet`
    pub fn new() -> BitSet {
        BitSet { blocks: Vec::new() }
    }

    /// Removes all entries from the set.
    pub fn clear(&mut self) {
        self.blocks.clear();
    }

    /// Inserts a bit into the set. Sets the entry at `idx` to `true`.
    pub fn insert(&mut self, idx: usize) {
        let (bit,block) = Self::bit_and_block(idx);
        *self.block_mut(block) |= bit;
    }

    /// Removes a bit from the set. Sets the entry at `idx` to `false`.
    pub fn remove(&mut self, idx: usize) {
        let (bit,block) = Self::bit_and_block(idx);
        if self.blocks.len() > block {
            *self.block_mut(block) &= !bit;
        }
    }

    /// Returns the value of the bit at `idx`
    pub fn get(&self, idx: usize) -> bool {
        let (bit,block) = Self::bit_and_block(idx);
        if self.block(block) & bit != 0 {
            return true;
        }
        false
    }

    /// Convert a bit index `idx` into an index into
    /// the block array and the corresponding bit value
    /// inside of that block.
    fn bit_and_block(idx: usize) -> (u64, usize) {
        const SHIFT64: usize = 6;
        const MASK64: usize = (1 << SHIFT64) - 1;
        let bit = (1usize << (idx & MASK64)) as u64;
        let block = idx >> SHIFT64;
        (bit, block)
    }

    /// Returns value stored at index `blk` or returns 0 if `blk`
    /// is index larger than block array.
    fn block(&self, blk: usize) -> u64 {
        if self.blocks.len() > blk {
            self.blocks[blk]
        } else {
            0
        }
    }

    /// Returns mutable reference to value stored at index `blk`
    /// and will resize block vector if index is larger than block
    /// array.
    fn block_mut(&mut self, blk: usize) -> &mut u64 {
        if self.blocks.len() <= blk {
            self.blocks.resize_with(blk + 1, Default::default);
        }
        &mut self.blocks[blk]
    }
}

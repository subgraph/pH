use std::ops::Index;

pub struct BitVec {
    blocks: Vec<u64>,
}

impl BitVec {
    pub fn new() -> BitVec {
        BitVec { blocks: Vec::new() }
    }

    pub fn set_bit(&mut self, idx: usize) {
        *self.mut_block(idx) |= Self::shifted_bit(idx)
    }

    pub fn clear_bit(&mut self, idx: usize) {
        if self.blocks.len() > Self::block_idx(idx) {
            let bit = Self::shifted_bit(idx);
            *self.mut_block(idx) &= !bit;
        }
    }

    pub fn get_bit(&self, n: usize) -> bool {
        let off = n % 64;
        let bit = 1 << off as u64;
        self.get_block(n) & bit != 0
    }

    pub fn first_unset(&self) -> usize {
        for (i,block) in self.blocks.iter().enumerate() {
            if *block != u64::max_value() {
                return (i * 64) + (0..64).find(|n| Self::shifted_bit(*n) & *block == 0).expect("...");
            }
        }
        self.blocks.len() * 64
    }

    fn shifted_bit(idx: usize) -> u64 {
        let shift = (idx % 64) as u64;
        (1 << shift)
    }

    fn block_idx(idx: usize) -> usize {
        idx / 64
    }

    fn get_block(&self, idx: usize) -> u64 {
        let idx = Self::block_idx(idx);
        if self.blocks.len() > idx {
            self.blocks[idx]
        } else {
            0
        }
    }

    fn mut_block(&mut self, idx: usize) -> &mut u64 {
        let idx = Self::block_idx(idx);
        if self.blocks.len() <= idx {
            self.blocks.resize_with(idx + 1, Default::default);
        }
        &mut self.blocks[idx]
    }
}

static TRUE: bool = true;
static FALSE: bool = false;

impl Index<usize> for BitVec {
    type Output = bool;

    fn index(&self, index: usize) -> &Self::Output {
        if self.get_bit(index) {
            &TRUE
        } else {
            &FALSE
        }
    }
}

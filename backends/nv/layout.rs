use crate::nv::error::LayoutError;

pub const HEADER_WORDS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Layout {
    depth: u32,
    capacity: u32,
    slot_words: usize,
    words: usize,
}

impl Layout {
    pub fn new(depth: u32, capacity: u32) -> Result<Self, LayoutError> {
        if depth < 2 || !depth.is_power_of_two() {
            return Err(LayoutError::InvalidDepth);
        }
        let payload_words = (capacity as usize)
            .checked_add(3)
            .ok_or(LayoutError::TooLarge)?
            / 4;
        let slot_words = HEADER_WORDS
            .checked_add(payload_words)
            .ok_or(LayoutError::TooLarge)?;
        let words = slot_words
            .checked_mul(depth as usize)
            .ok_or(LayoutError::TooLarge)?;
        if words > isize::MAX as usize / size_of::<u32>() {
            return Err(LayoutError::TooLarge);
        }
        Ok(Self {
            depth,
            capacity,
            slot_words,
            words,
        })
    }

    pub const fn depth(self) -> u32 {
        self.depth
    }

    pub const fn capacity(self) -> u32 {
        self.capacity
    }

    pub const fn slot_words(self) -> usize {
        self.slot_words
    }

    pub const fn words(self) -> usize {
        self.words
    }

    pub fn init(self, arena: &mut [u32]) {
        assert_eq!(arena.len(), self.words);
        arena.fill(0);
        for slot in 0..self.depth as usize {
            arena[slot * self.slot_words] = slot as u32;
        }
    }

    #[inline(always)]
    pub fn slot(self, seq: u32) -> usize {
        (seq & (self.depth - 1)) as usize * self.slot_words
    }
}

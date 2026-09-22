use super::*;

pub struct Rx {
    ptr: *mut u32,
    layout: Layout,
    seq: u32,
}

impl Rx {
    /// Creates one lane's view of a directed link.
    ///
    /// # Safety
    ///
    /// Every lane must pass the same aligned global-memory `ptr` and `layout`.
    /// The pointer must cover `layout.words()` words initialized by
    /// `Layout::init` for the endpoint's lifetime and match exactly one sender
    /// on the same CUDA device. Its arena must not overlap another live link or
    /// any concurrent ordinary access. State words may only be accessed
    /// through the transport's atomics while either endpoint is live. Exactly
    /// one full warp may receive from it, in convergence.
    #[inline(always)]
    pub unsafe fn new(ptr: *mut u32, layout: Layout) -> Self {
        Self {
            ptr,
            layout,
            seq: 0,
        }
    }

    /// Attempts one warp-cooperative receive.
    ///
    /// A too-small output does not consume the message.
    ///
    /// # Safety
    ///
    /// All lanes must call this in convergence with the same valid `out` and
    /// `capacity`. The output may not overlap this link's arena.
    #[inline(always)]
    pub unsafe fn recv(&mut self, out: *mut u8, capacity: u32) -> Result<Message, RecvError> {
        let lane = warp::lane_id();
        let slot = self.layout.slot(self.seq);
        let state_ptr = unsafe { self.ptr.add(slot) };
        let observed = if lane == 0 {
            unsafe { DeviceAtomicU32::from_ptr(state_ptr) }.load(AtomicOrdering::Acquire)
        } else {
            0
        };
        if warp::shuffle(observed, 0) != self.seq.wrapping_add(1) {
            return Err(RecvError::Empty);
        }

        warp::sync_mask(WARP);
        let len = warp::shuffle(
            if lane == 0 {
                unsafe { self.ptr.add(slot + 1).read() }
            } else {
                0
            },
            0,
        );
        let src = warp::shuffle(
            if lane == 0 {
                unsafe { self.ptr.add(slot + 2).read() }
            } else {
                0
            },
            0,
        );
        let tag = warp::shuffle(
            if lane == 0 {
                unsafe { self.ptr.add(slot + 3).read() }
            } else {
                0
            },
            0,
        );
        if capacity < len {
            return Err(RecvError::TooSmall { needed: len });
        }

        let payload_words = len.div_ceil(4);
        let mut word = lane;
        // As `Tx::send`: four loads issued before the stores, so a lane keeps several reads in
        // flight rather than paying one L2 round trip per word.
        while word + 96 < payload_words {
            let a = unsafe { self.ptr.add(slot + 4 + word as usize).read() };
            let b = unsafe { self.ptr.add(slot + 4 + (word + 32) as usize).read() };
            let c = unsafe { self.ptr.add(slot + 4 + (word + 64) as usize).read() };
            let d = unsafe { self.ptr.add(slot + 4 + (word + 96) as usize).read() };
            unsafe {
                write_word(out, len, word, a);
                write_word(out, len, word + 32, b);
                write_word(out, len, word + 64, c);
                write_word(out, len, word + 96, d);
            }
            word += 128;
        }
        while word < payload_words {
            let value = unsafe { self.ptr.add(slot + 4 + word as usize).read() };
            unsafe { write_word(out, len, word, value) };
            word += 32;
        }
        warp::sync_mask(WARP);
        if lane == 0 {
            unsafe { DeviceAtomicU32::from_ptr(state_ptr) }.store(
                self.seq.wrapping_add(self.layout.depth()),
                AtomicOrdering::Release,
            );
        }
        warp::sync_mask(WARP);
        self.seq = self.seq.wrapping_add(1);
        Ok(Message { src, tag, len })
    }
}

use super::*;
use cuda_device::atomic::SystemAtomicU32;

pub struct Tx {
    ptr: *mut u32,
    layout: Layout,
    src: u32,
    seq: u32,
}

impl Tx {
    /// Creates one lane's view of a directed link.
    ///
    /// # Safety
    ///
    /// Every lane must pass the same aligned global-memory `ptr` and `layout`.
    /// The pointer must cover `layout.words()` words initialized by
    /// `Layout::init` for the endpoint's lifetime. Its arena must not overlap
    /// another live link or any concurrent ordinary access.
    /// Exactly one full warp may send on the link, and its matching receiver
    /// must use the same pointer and layout on the same CUDA device. State
    /// words may only be accessed through the transport's atomics while either
    /// endpoint is live. Every lane must call `try_send` in convergence with
    /// identical arguments.
    #[inline(always)]
    pub unsafe fn new(ptr: *mut u32, layout: Layout, src: u32) -> Self {
        unsafe { Self::resume(ptr, layout, src, 0) }
    }

    /// The endpoint of a link whose sequence is held apart from its pointer, resumed for one call;
    /// [`Tx::seq`] is what to hold after it. `CudaTransport` holds its links this way because
    /// cuda-oxide cannot lower an array of pointer-bearing endpoints inside an enum's payload, and
    /// `init` returns its context in a `Result`.
    ///
    /// # Safety
    ///
    /// As [`Tx::new`], and `seq` is the sequence this link's sender last reached.
    #[inline(always)]
    pub(crate) unsafe fn resume(ptr: *mut u32, layout: Layout, src: u32, seq: u32) -> Self {
        Self {
            ptr,
            layout,
            src,
            seq,
        }
    }

    #[inline(always)]
    pub(crate) fn seq(&self) -> u32 {
        self.seq
    }

    /// Attempts one warp-cooperative send.
    ///
    /// # Safety
    ///
    /// All lanes must call this in convergence with the same valid `data` and
    /// `len`. The memory may not overlap this link's arena.
    #[inline(always)]
    pub unsafe fn send(&mut self, tag: u32, data: *const u8, len: u32) -> Result<(), SendError> {
        if len > self.layout.capacity() {
            return Err(SendError::TooLarge);
        }

        let lane = warp::lane_id();
        let slot = self.layout.slot(self.seq);
        let state_ptr = unsafe { self.ptr.add(slot) };
        let observed = if lane == 0 {
            // System scope: the leader end of this link is a CPU thread, and device scope does not order against it.
            unsafe { SystemAtomicU32::from_ptr(state_ptr) }.load(AtomicOrdering::Acquire)
        } else {
            0
        };
        if warp::shuffle(observed, 0) != self.seq {
            return Err(SendError::Full);
        }

        warp::sync_mask(WARP);
        if lane == 0 {
            unsafe {
                self.ptr.add(slot + 1).write(len);
                self.ptr.add(slot + 2).write(self.src);
                self.ptr.add(slot + 3).write(tag);
            }
        }
        let payload_words = len.div_ceil(4);
        let mut word = lane;
        // Four words are loaded before any is stored, so four memory operations are in flight on
        // this lane instead of one. The kernel has one warp per scheduler and issues an instruction
        // every ~11 cycles, so outstanding-miss count is the only lever the bulk path has left:
        // with a single operation in flight the lane costs one full L2 round trip per word.
        while word + 96 < payload_words {
            let a = unsafe { read_word(data, len, word) };
            let b = unsafe { read_word(data, len, word + 32) };
            let c = unsafe { read_word(data, len, word + 64) };
            let d = unsafe { read_word(data, len, word + 96) };
            unsafe {
                self.ptr.add(slot + 4 + word as usize).write(a);
                self.ptr.add(slot + 4 + (word + 32) as usize).write(b);
                self.ptr.add(slot + 4 + (word + 64) as usize).write(c);
                self.ptr.add(slot + 4 + (word + 96) as usize).write(d);
            }
            word += 128;
        }
        while word < payload_words {
            let value = unsafe { read_word(data, len, word) };
            unsafe { self.ptr.add(slot + 4 + word as usize).write(value) };
            word += 32;
        }
        warp::sync_mask(WARP);
        if lane == 0 {
            unsafe { SystemAtomicU32::from_ptr(state_ptr) }
                .store(self.seq.wrapping_add(1), AtomicOrdering::Release);
        }
        warp::sync_mask(WARP);
        self.seq = self.seq.wrapping_add(1);
        Ok(())
    }
}

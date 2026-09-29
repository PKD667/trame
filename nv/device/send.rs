use super::*;
use cuda_device::atomic::SystemAtomicU32;

pub struct Tx {
    ptr: *mut u32,
    layout: Layout,
    src: u32,
    seq: u32,
}

impl Tx {
    /// Creates the scalar owner's view of a directed link.
    ///
    /// # Safety
    ///
    /// `ptr` covers an aligned global-memory ring initialized by `Layout::init`,
    /// alive for this endpoint. Exactly one scalar owner sends, and one receives.
    /// The ring must not overlap caller buffers or ordinary concurrent access.
    /// State words are atomic-only while endpoints are live. A CPU endpoint
    /// additionally requires admitted CPU/GPU-visible storage.
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

    /// Attempts one scalar send.
    ///
    /// # Safety
    ///
    /// `data` is readable for `len` bytes by this owner and does not overlap the ring.
    #[inline(always)]
    pub unsafe fn send(&mut self, tag: u32, data: *const u8, len: u32) -> Result<(), SendError> {
        if len > self.layout.capacity() {
            return Err(SendError::TooLarge);
        }

        let slot = self.layout.slot(self.seq);
        let state_ptr = unsafe { self.ptr.add(slot) };
        // System scope also serves a CPU leader when this is a mapped ring.
        let state = unsafe { SystemAtomicU32::from_ptr(state_ptr) };
        if state.load(AtomicOrdering::Acquire) != self.seq {
            return Err(SendError::Full);
        }
        unsafe {
            self.ptr.add(slot + 1).write(len);
            self.ptr.add(slot + 2).write(self.src);
            self.ptr.add(slot + 3).write(tag);
        }
        let mut word = 0;
        while word < len.div_ceil(4) {
            let value = unsafe { read_word(data, len, word) };
            unsafe { self.ptr.add(slot + 4 + word as usize).write(value) };
            word += 1;
        }
        state.store(self.seq.wrapping_add(1), AtomicOrdering::Release);
        self.seq = self.seq.wrapping_add(1);
        Ok(())
    }
}

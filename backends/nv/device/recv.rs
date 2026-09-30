use super::*;
use cuda_device::atomic::SystemAtomicU32;

pub struct Rx {
    ptr: *mut u32,
    layout: Layout,
    seq: u32,
}

impl Rx {
    /// Creates the scalar owner's view of a directed link.
    ///
    /// # Safety
    ///
    /// `ptr` covers an aligned global-memory ring initialized by `Layout::init`,
    /// alive for this endpoint and matched to exactly one scalar sender.
    /// One scalar owner receives. The ring does not overlap ordinary concurrent
    /// access or output buffers; state words are atomic-only while endpoints live.
    /// A CPU endpoint additionally requires admitted CPU/GPU-visible storage.
    #[inline(always)]
    pub unsafe fn new(ptr: *mut u32, layout: Layout) -> Self {
        unsafe { Self::resume(ptr, layout, 0) }
    }

    /// The endpoint resumed at sequence `seq`, for the reason [`Tx::resume`](super::Tx::resume)
    /// gives.
    ///
    /// # Safety
    ///
    /// As [`Rx::new`], and `seq` is the sequence this link's receiver last reached.
    #[inline(always)]
    pub(crate) unsafe fn resume(ptr: *mut u32, layout: Layout, seq: u32) -> Self {
        Self { ptr, layout, seq }
    }

    #[inline(always)]
    pub(crate) fn seq(&self) -> u32 {
        self.seq
    }

    /// The next published tag, without consuming the frame.
    ///
    /// # Safety
    /// The owner holds the sole receiving endpoint of the ring.
    #[inline(always)]
    pub unsafe fn head(&self) -> Option<u32> {
        let slot = self.layout.slot(self.seq);
        let state = unsafe { SystemAtomicU32::from_ptr(self.ptr.add(slot)) };
        if state.load(AtomicOrdering::Acquire) != self.seq.wrapping_add(1) {
            return None;
        }
        Some(unsafe { self.ptr.add(slot + 3).read() })
    }

    /// One scalar receive; a short output consumes nothing.
    ///
    /// # Safety
    /// `out` is writable for `capacity` bytes by this owner and does not overlap the ring.
    #[inline(always)]
    pub unsafe fn recv(&mut self, out: *mut u8, capacity: u32) -> Result<Message, RecvError> {
        let slot = self.layout.slot(self.seq);
        let state = unsafe { SystemAtomicU32::from_ptr(self.ptr.add(slot)) };
        if state.load(AtomicOrdering::Acquire) != self.seq.wrapping_add(1) {
            return Err(RecvError::Empty);
        }
        let len = unsafe { self.ptr.add(slot + 1).read() };
        let src = unsafe { self.ptr.add(slot + 2).read() };
        let tag = unsafe { self.ptr.add(slot + 3).read() };
        if capacity < len {
            return Err(RecvError::TooSmall { needed: len });
        }
        let mut word = 0;
        while word < len.div_ceil(4) {
            let value = unsafe { self.ptr.add(slot + 4 + word as usize).read() };
            unsafe { write_word(out, len, word, value) };
            word += 1;
        }
        state.store(self.seq.wrapping_add(self.layout.depth()), AtomicOrdering::Release);
        self.seq = self.seq.wrapping_add(1);
        Ok(Message { src, tag, len })
    }
}

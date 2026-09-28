//! Device transport: each rank is one warp over `device::{Tx, Rx}` rings.
//!
//! Mirrors the pingpong arena: `size × size` rings, link (src, dst) at
//! `(src * size + dst) * layout.words()` words into `arena`.
//!
//! Not compiled or exercised without the CUDA toolkit; the host-side sim is
//! the verified path.

use super::{MAX_RANKS, Transport};
use crate::nv::device::{Rx, Tx};
use crate::nv::error::{RecvError, SendError};
use crate::nv::layout::Layout;
use crate::nv::transport::Message;

/// One rank's endpoints into a `size × size` ring arena.
///
/// It holds the arena and each link's sequence, not an array of `Tx`/`Rx`: every endpoint carries
/// a pointer, cuda-oxide refuses an array of pointers inside an enum's payload, and this struct is
/// inside `Result<Context, Failure>`. An endpoint is rebuilt from the arena and its sequence for
/// each call.
pub struct CudaTransport {
    rank: u32,
    size: u32,
    arena: *mut u32,
    layout: Layout,
    /// The sequence each outgoing link, by destination, has reached.
    sent: [u32; MAX_RANKS],
    /// The sequence each incoming link, by source, has reached.
    received: [u32; MAX_RANKS],
}

impl CudaTransport {
    /// # Safety
    ///
    /// `arena` must cover `size * size * layout.words()` words of aligned
    /// global memory, each ring initialized by `Layout::init`, alive for the
    /// endpoints' lifetime and not overlapping any concurrent access. Every
    /// lane of rank `rank` must call this with the same pointer and layout.
    pub unsafe fn new(arena: *mut u32, layout: Layout, size: u32, rank: u32) -> Self {
        assert!(size as usize <= MAX_RANKS);
        Self {
            rank,
            size,
            arena,
            layout,
            sent: [0; MAX_RANKS],
            received: [0; MAX_RANKS],
        }
    }

    /// Link (src, dst)'s ring.
    fn ring(&self, src: u32, dst: u32) -> *mut u32 {
        assert!(src < self.size && dst < self.size);
        // SAFETY: both ends are below `size`, so the ring is inside the arena `new` was given.
        unsafe { self.arena.add((src * self.size + dst) as usize * self.layout.words()) }
    }
}

impl Transport for CudaTransport {
    fn rank(&self) -> u32 {
        self.rank
    }

    fn size(&self) -> u32 {
        self.size
    }

    fn try_send(&mut self, dst: u32, tag: u32, data: &[u8]) -> Result<(), SendError> {
        let len = u32::try_from(data.len()).map_err(|_| SendError::TooLarge)?;
        let ring = self.ring(self.rank, dst);
        // SAFETY: `new`'s contract covers the ring, and `sent[dst]` is where this link's sender
        // stopped.
        let mut tx = unsafe { Tx::resume(ring, self.layout, self.rank, self.sent[dst as usize]) };
        let sent = unsafe { tx.send(tag, data.as_ptr(), len) };
        self.sent[dst as usize] = tx.seq();
        sent
    }

    fn try_recv(&mut self, src: u32, out: &mut [u8]) -> Result<Message, RecvError> {
        // A buffer beyond `u32::MAX` bytes holds any frame a slot can, so its room saturates.
        let room = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let ring = self.ring(src, self.rank);
        // SAFETY: as in `try_send`, for the receiving end.
        let mut rx = unsafe { Rx::resume(ring, self.layout, self.received[src as usize]) };
        let received = unsafe { rx.recv(out.as_mut_ptr(), room) };
        self.received[src as usize] = rx.seq();
        received
    }

    fn barrier(&mut self, members: u32) {
        let rings = (self.size * self.size) as usize * self.layout.words();
        unsafe { crate::nv::device::barrier(self.arena.add(rings), members) }
    }

    fn head(&mut self, src: u32) -> Option<u32> {
        let ring = self.ring(src, self.rank);
        let rx = unsafe { Rx::resume(ring, self.layout, self.received[src as usize]) };
        unsafe { rx.head() }
    }
}

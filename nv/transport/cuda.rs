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
pub struct CudaTransport {
    rank: u32,
    size: u32,
    tx: [Tx; MAX_RANKS],
    rx: [Rx; MAX_RANKS],
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
        let tx = std::array::from_fn(|dst| unsafe {
            let ptr = arena.add((rank * size + dst as u32) as usize * layout.words());
            Tx::new(ptr, layout, rank)
        });
        let rx = std::array::from_fn(|src| unsafe {
            let ptr = arena.add((src as u32 * size + rank) as usize * layout.words());
            Rx::new(ptr, layout)
        });
        Self { rank, size, tx, rx }
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
        unsafe { self.tx[dst as usize].send(tag, data.as_ptr(), data.len() as u32) }
    }

    fn try_recv(&mut self, src: u32, out: &mut [u8]) -> Result<Message, RecvError> {
        unsafe { self.rx[src as usize].recv(out.as_mut_ptr(), out.len() as u32) }
    }
}

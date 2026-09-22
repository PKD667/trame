//! The device's peers: this participant's endpoints over the launch's ring arena.
//!
//! The arena is the launch's, built before it starts, and `Fabric` is where the obligation to
//! describe it correctly is discharged — once, by whoever allocated it, which is why constructing
//! one is `unsafe` and opening the endpoints is not. Every warp calls `open` with the same fabric
//! and its own rank, and each gets its own endpoints: the arena is indexed by rank, so a warp's
//! senders are its own row and its receivers are its own column, and no two participants write the
//! same link.
//!
//! The endpoints carry the sequence each link has reached, which is the slot the next send goes
//! into. That is why they cannot be rebuilt per call and why they are participant-local rather
//! than a global: a sequence number restarted under a receiver that has moved past it lands every
//! frame after the first in the wrong slot, and a sequence number shared between participants is
//! two senders advancing one counter.

use super::{Refused, refused_recv, refused_send};
use crate::nv::layout::Layout;
use crate::nv::transport::Message;
use crate::nv::transport::Transport;
use crate::nv::transport::cuda::CudaTransport;

/// The launch's ring arena, described once.
///
/// # Safety
///
/// The caller of [`Fabric::new`] promises that `arena` covers
/// `size * size * layout.words()` words of aligned device global memory, one ring per ordered pair
/// at `(src * size + dst) * layout.words()`, each already initialized by [`Layout::init`]; that it
/// stays alive and unreserved for the whole launch; and that no ordinary access overlaps it.
#[derive(Clone, Copy)]
pub struct Fabric {
    arena: *mut u32,
    layout: Layout,
}

// SAFETY: the arena is device global memory the launch owns and every participant reads a distinct
// row and column of it, so sharing the description between participants does not share a location
// they write.
unsafe impl Sync for Fabric {}
unsafe impl Send for Fabric {}

impl Fabric {
    /// # Safety
    ///
    /// See the type's contract. It is the whole of what this backend needs from its launcher.
    pub unsafe fn new(arena: *mut u32, layout: Layout) -> Self {
        Fabric { arena, layout }
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }
}

/// One participant's endpoints. Participant-local, and the reason a launch cannot keep them in a
/// shared slot.
pub struct Links {
    rank: u32,
    size: u32,
    layout: Layout,
    transport: CudaTransport,
}

impl Links {
    pub fn open(fabric: &Fabric, rank: u32, size: u32) -> Result<Links, Refused> {
        if rank >= size {
            return Err(Refused::NoSuchPeer);
        }
        // SAFETY: the arena contract was discharged when the fabric was built, and this rank's row
        // and column lie inside it by the same contract.
        let transport = unsafe { CudaTransport::new(fabric.arena, fabric.layout, size, rank) };
        Ok(Links {
            rank,
            size,
            layout: fabric.layout,
            transport,
        })
    }

    pub fn rank(&self) -> u32 {
        self.rank
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn send(&mut self, dest: u32, tag: u32, data: &[u8]) -> Result<(), Refused> {
        if dest >= self.size {
            return Err(Refused::NoSuchPeer);
        }
        self.transport
            .try_send(dest, tag, data)
            .map_err(refused_send)
    }

    pub fn recv(&mut self, src: u32, out: &mut [u8]) -> Result<Message, Refused> {
        if src >= self.size {
            return Err(Refused::NoSuchPeer);
        }
        self.transport.try_recv(src, out).map_err(refused_recv)
    }
}

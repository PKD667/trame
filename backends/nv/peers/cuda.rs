//! The device's peers: this participant's endpoints over the launch's ring arena.
//!
//! The arena is the launch's, built before it starts and stated in the launch description, and the
//! obligation that it is what the description says is discharged once, by the launcher's write of
//! that description; `Fabric::described` checks what can be checked, and opening the endpoints is
//! safe. Every warp calls `open` with the same fabric
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
use crate::contract::{BackendFault, Invalid};
use crate::nv::layout::Layout;
use crate::nv::transport::Message;
use crate::nv::transport::Transport;
use crate::nv::transport::cuda::CudaTransport;

/// The arena as the launch description states it: where it starts and how many words it has.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Arena {
    pub base: *mut u32,
    pub words: usize,
}

/// The launch's ring arena, described once.
///
/// What [`Fabric::described`] checks is that the description is consistent: a non-null, aligned
/// base and at least `size * size * layout.words()` words. What it cannot check is the launcher's
/// obligation, stated at the launcher's write of the description: that those words are device
/// global memory, one ring per ordered pair at `(src * size + dst) * layout.words()`, each already
/// initialized by [`Layout::init`], alive and unreserved for the whole launch, and that no ordinary
/// access overlaps them.
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
    /// The fabric the launch description states, or why it is not one: an arena that is null or
    /// misaligned disagrees with itself, and one too short for `size` ranks is too little storage.
    pub fn described(arena: Arena, layout: Layout, size: u32) -> Result<Fabric, BackendFault> {
        if arena.base.is_null() || !arena.base.is_aligned() {
            return Err(BackendFault::Invalid(Invalid::InconsistentLaunch));
        }
        let needed = (size as usize)
            .checked_mul(size as usize)
            .and_then(|pairs| pairs.checked_mul(layout.words()))
            .and_then(|rings| rings.checked_add(super::BARRIER))
            .ok_or(BackendFault::Storage)?;
        if arena.words < needed {
            return Err(BackendFault::Storage);
        }
        Ok(Fabric {
            arena: arena.base,
            layout,
        })
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
        // SAFETY: the launcher's write of the description discharged the arena contract, `described`
        // checked the arena holds `size * size` rings, and `rank < size` puts this rank's row and
        // column inside them.
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

    /// Wait until `members` participants of the launch have called this.
    pub fn barrier(&mut self, members: u32) {
        self.transport.barrier(members)
    }

    /// The tag of the frame `recv` from `src` would take next, if one is published.
    pub fn head(&mut self, src: u32) -> Option<u32> {
        if src >= self.size {
            return None;
        }
        self.transport.head(src)
    }
}

//! The launch's peers: the endpoints one participant uses to reach the others.
//!
//! A value the participant owns, not a global. What was here before was worse than wrong: every
//! warp of a launch wrote its own rank into one shared word and its own endpoints into one shared
//! cell, so the last writer won and every warp then used another warp's identity and another
//! warp's links. Concurrent writes through an aliased `UnsafeCell` are also undefined behaviour in
//! the Rust model, and the values differing per participant meant it did not even have the defence
//! that identical bytes were being written.
//!
//! The endpoints are participant-local state — a ring's sequence number is the slot the next send
//! goes into, so two participants cannot share one — and participant-local state belongs in the
//! participant's context, passed by `&mut`, which is what the contract requires. There is no
//! global here to race with, and a second participant in the same process cannot inherit the first
//! one's identity because there is nowhere for it to be inherited from.
//!
//! Two halves, one interface, chosen at compile time exactly as `transport` chooses between `sim`
//! and `cuda`. [`Links::open`] is safe on both because the obligation belongs to [`Fabric`]:
//! constructing one from raw device memory is `unsafe`, and on the host model it is an ordinary
//! allocation with nothing to uphold.

use crate::contract::{Rank, Tag};
use crate::nv::error::{RecvError, SendError};
use crate::nv::layout::Layout;
use crate::nv::transport::Message;

#[cfg(feature = "cuda")]
mod cuda;
#[cfg(not(feature = "cuda"))]
mod sim;

#[cfg(feature = "cuda")]
pub use cuda::{Fabric, Links};

#[cfg(not(feature = "cuda"))]
pub use sim::{Fabric, Links};

/// One send or receive that the links could not perform.
///
/// A thin union of the two transport errors rather than a restatement of them: the transport's
/// refusals are already the distinctions a caller acts on, and a second vocabulary for the same
/// two facts would be a second place to keep them in step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// No slot at this instant.
    Full,
    /// The frame is larger than a slot.
    TooLarge,
    /// Nothing waiting.
    Empty,
    /// The caller's buffer is smaller than the frame; nothing was consumed.
    TooSmall { needed: u32 },
    /// The destination is not a participant of this launch.
    NoSuchPeer,
}

/// Send one frame into `dest`'s slot, if there is one free.
///
/// `data` is copied before this returns, so the caller's borrow ends here. The whole warp executes
/// one call: the payload is copied lane-strided, which is why the pointer and the length have to
/// be the same in every lane.
pub fn try_send(links: &mut Links, dest: Rank, tag: Tag, data: &[u8]) -> Result<(), Refused> {
    links.send(dest, tag, data)
}

/// Take one frame from `src`, if one is ready. `out` is written lane-strided, so its pointer and
/// length are uniform across the warp for the same reason.
pub fn try_recv(links: &mut Links, src: Rank, out: &mut [u8]) -> Result<Message, Refused> {
    links.recv(src, out)
}

/// The layout the launch's links were built with. One layout covers every pair, because the arena
/// is allocated before any graph is known.
pub fn layout(links: &Links) -> Layout {
    links.layout()
}

/// Map the send transport's refusal onto this module's.
#[inline]
pub(crate) fn refused_send(error: SendError) -> Refused {
    match error {
        SendError::Full => Refused::Full,
        SendError::TooLarge => Refused::TooLarge,
        // The same meaning the peer module already has for it: an address that names nobody.
        SendError::NoSuchRank => Refused::NoSuchPeer,
    }
}

/// Map the receive transport's refusal onto this module's.
#[inline]
pub(crate) fn refused_recv(error: RecvError) -> Refused {
    match error {
        RecvError::Empty => Refused::Empty,
        RecvError::TooSmall { needed } => Refused::TooSmall { needed },
    }
}

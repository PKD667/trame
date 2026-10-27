//! Typed messages over routes: spec.md sections 3 ("Messages") and 5.2. A message type is also the
//! tag of its routes, so one route family per type.

use crate::backend::{Backend, Selected};
use crate::topology::WorkerId;
use alloc::sync::Arc;
use core::any::{type_name, TypeId};

/// A message type: plain data, sent as its bytes. Every `Pod` type is one (bytemuck: no padding, no
/// pointer or reference, every bit pattern a value), so numbers, arrays of them and structs of them
/// are messages. `#[trame::message]` makes a struct `Pod`. Message bytes are little-endian.
///
/// The supertrait is the check: a type that is not `Pod` cannot be a message, and no impl but the
/// blanket one below is needed or possible.
///
/// # Safety
/// Holds for every `Pod` type, by the safety of `Pod`.
pub unsafe trait Message: crate::__bytemuck::Pod + 'static {
    /// The all-zero value of the type, the start of a `read`.
    fn zeroed() -> Self {
        <Self as crate::__bytemuck::Zeroable>::zeroed()
    }
}

// SAFETY: a `Pod` type is plain data (bytemuck's contract), which is what `Message` promises.
unsafe impl<T: crate::__bytemuck::Pod + 'static> Message for T {}

/// A send answered `full`: the route is at capacity and is not lossy (spec.md 5.2, Full).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// Send `msg` from this worker to `to`, on the route of type `M`. Answers `Full` only when that
/// route is not lossy and at capacity. Panics when the topology has no such route.
pub fn send<M: Message>(to: WorkerId, msg: M) -> Result<(), Full> {
    Selected::worker_send(to, TypeId::of::<M>(), type_name::<M>(), crate::__bytemuck::bytes_of(&msg))
}

/// The next message of type `M` addressed to this worker, or `None` when none is available. Inside a
/// step of `concurrent!`, a type the process does not own answers `None` (spec.md 5.2, Ownership).
pub fn recv<M: Message>() -> Option<M> {
    if !crate::cells::owns(TypeId::of::<M>()) {
        return None;
    }
    read(|out| Selected::worker_recv(TypeId::of::<M>(), out))
}

/// Wait until every worker is at a barrier, then return a snapshot of the host's publication (spec.md
/// 5.5). The snapshot shares the bytes with the backend, and the worker may keep it.
pub fn barrier() -> Arc<[u8]> {
    Selected::worker_barrier()
}

/// One message of type `M`, read by `take` into a value of `M` that starts zeroed. `take` answers
/// whether it wrote a frame. Every bit pattern is a value of `M` (`Message`), so the bytes the
/// backend wrote are a valid `M`, and the frame's size is `size_of::<M>()` by the topology check.
pub(crate) fn read<M: Message>(take: impl FnOnce(&mut [u8]) -> bool) -> Option<M> {
    let mut out = <M as Message>::zeroed();
    if take(crate::__bytemuck::bytes_of_mut(&mut out)) {
        Some(out)
    } else {
        None
    }
}

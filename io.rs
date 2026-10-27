//! Typed messages over routes: spec.md sections 3 ("Messages") and 5.2. A message type is also the
//! tag of its routes, so one route family per type.

use crate::backend::{Backend, Selected};
use crate::topology::WorkerId;
use alloc::sync::Arc;
use core::any::{type_name, TypeId};

/// A message type: plain data, sent as its bytes.
///
/// The bounds are the checks: `NoUninit` holds only for a type with no padding, and `AnyBitPattern`
/// only for a type whose every bit pattern is a value. Implement it with `#[trame::message]`,
/// which also refuses references and raw pointers, since bytemuck accepts a raw pointer as plain
/// data. A hand-written impl is the implementer's promise that the type holds no reference, pointer,
/// `Lock` or route handle, and that no field hides one behind an alias.
///
/// # Safety
/// The implementer promises that `Self` holds no reference, pointer, `Lock` or route handle.
pub unsafe trait Message: crate::__bytemuck::NoUninit + crate::__bytemuck::AnyBitPattern + 'static {}

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
    let mut out = <M as crate::__bytemuck::Zeroable>::zeroed();
    if take(crate::__bytemuck::bytes_of_mut(&mut out)) {
        Some(out)
    } else {
        None
    }
}

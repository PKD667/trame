//! Typed messages over routes: INTERFACE.md sections 3 ("Messages") and 5.2. A message type is also the
//! tag of its routes, so one route family per type.

use crate::backend::{Backend, Selected};
use core::any::{TypeId, type_name};

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

/// A send answered `full`: the route is at capacity and is not lossy (INTERFACE.md 5.2, Full).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// What a route from a worker to the leader carries: the message, and the sender's clock when it was sent
/// (INTERFACE.md 5.2, Definition (Stamp)). Trame makes it and takes it apart; a program sees `Stamped`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub(crate) struct Wire<M> {
    pub(crate) clock: u64,
    pub(crate) nanos: u64,
    pub(crate) value: M,
}

// SAFETY: `repr(C, packed)` has no padding, `M` is plain data, and a clock and a count of nanoseconds are
// valid in every bit pattern: the struct is its bytes.
unsafe impl<M: crate::__bytemuck::Zeroable> crate::__bytemuck::Zeroable for Wire<M> {}
// SAFETY: as above; `Pod` also needs `Copy + 'static`, which the derive and `M: Pod` give.
unsafe impl<M: crate::__bytemuck::Pod> crate::__bytemuck::Pod for Wire<M> {}

/// A message from a worker, with the reading of the worker's clock at its send: what `Run::recv` answers.
/// The data dates from `at`; two stamps of one worker give the time that passed on that worker between
/// them, and stamps of two workers do not compare (INTERFACE.md 5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamped<M> {
    pub at: crate::time::Reading,
    pub value: M,
}

/// Send `msg` from this worker to `to`, on the route of type `M`. Answers `Full` only when that
/// route is not lossy and at capacity. Panics when the topology has no such route. A message to the leader
/// is stamped with this worker's clock (INTERFACE.md 5.2).
pub fn send<M: Message>(to: impl Into<crate::topology::End>, msg: M) -> Result<(), Full> {
    let to = to.into();
    if to == crate::topology::End::Leader {
        let (clock, nanos) = Selected::worker_stamp();
        // The bytes of a `Wire<M>`, written in place: a large message is not copied onto the stack a second time.
        let body = crate::__bytemuck::bytes_of(&msg);
        let mut small = [0u8; 4112];
        let mut big;
        let frame: &mut [u8] = if body.len() <= 4096 {
            &mut small[..16 + body.len()]
        } else {
            big = alloc::vec![0u8; 16 + body.len()];
            &mut big[..]
        };
        frame[..8].copy_from_slice(&clock.raw().to_le_bytes());
        frame[8..16].copy_from_slice(&nanos.to_le_bytes());
        frame[16..].copy_from_slice(body);
        return Selected::worker_send(to, TypeId::of::<Wire<M>>(), type_name::<M>(), frame);
    }
    Selected::worker_send(to, TypeId::of::<M>(), type_name::<M>(), crate::__bytemuck::bytes_of(&msg))
}

/// The next message of type `M` addressed to this worker, or `None` when none is available. A type
/// the running step does not receive answers `None` (INTERFACE.md 5.2, Ownership).
pub fn recv<M: Message>() -> Option<M> {
    if !crate::cells::owns(TypeId::of::<M>()) {
        return None;
    }
    read(|out| Selected::worker_recv(TypeId::of::<M>(), out))
}

/// One message of type `M`, read by `take` into a value of `M` that starts zeroed. `take` answers
/// whether it wrote a frame. Every bit pattern is a value of `M` (`Message`), so the bytes the
/// backend wrote are a valid `M`, and the frame's size is `size_of::<M>()` by the topology check.
pub(crate) fn read<M: Message>(take: impl FnOnce(&mut [u8]) -> bool) -> Option<M> {
    let mut out = <M as Message>::zeroed();
    if take(crate::__bytemuck::bytes_of_mut(&mut out)) { Some(out) } else { None }
}

/// This host (`HostId`), on its leader and in its workers.
pub fn host() -> crate::topology::HostId {
    crate::topology::HostId(Selected::host())
}

/// The number of hosts of the deployment. Hosts are numbered from 0, so `HostId(h)` names a peer for
/// every `h < hosts()`; membership is fixed for the run (CONTRACT.md, Federation). A backend whose deployment is one host answers 1.
pub fn hosts() -> u32 {
    Selected::hosts()
}

/// The number of workers of host `h`, the leader not counted (INTERFACE.md 6), or `None` when this process does
/// not know it. A launcher may supply the counts before the hosts have discovered each other; once they have
/// (`launch` has returned, or the first `host()` or `hosts()` on a backend that discovers there), every host's count is
/// known. `None` also for a number that is no host.
pub fn workers(h: crate::topology::HostId) -> Option<u32> {
    Selected::workers(h.0)
}

/// Whether this process is its host's leader (CONTRACT.md F1, F6). Only a leader sends and receives peer messages.
/// A backend whose host is one process is its own leader. Answers once the hosts have discovered each other.
pub fn is_leader() -> bool {
    Selected::is_leader()
}

/// Block until a peer message may have arrived for this leader (returns at once when one is held): the wait before a
/// launch, where `Run::wait` does not exist yet. Only a leader calls this.
pub fn peer_wait() {
    Selected::peer_wait()
}

/// The tag of a message type across hosts (CONTRACT.md F4): equal in two compilations of the same
/// type, unlike `TypeId`. FNV-1a over the type's path and its size.
pub(crate) fn tag<M: Message>() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in type_name::<M>().bytes().chain((core::mem::size_of::<M>() as u64).to_le_bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// This worker. Panics outside a worker.
pub fn me() -> crate::topology::WorkerId {
    Selected::worker_me()
}

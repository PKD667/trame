// The worker's side: every operation acts for this process's worker endpoint (state.rs). Each one
// first checks for an abort. An aborted run ends this worker at its next effect (spec.md 5, Abort):
// the check unwinds with `Aborted`, which the worker's catch_unwind (imp.rs) records as Abort.

extern crate std;

use core::any::TypeId;
use std::boxed::Box;
use std::panic::resume_unwind;

use crate::io::Full;
use crate::time::ClockId;
use std::sync::Arc;
use crate::topology::WorkerId;

use super::plan::{self, rank_of, Entry};
use super::state::local;
use super::types::{Answer, Error, Received};
use super::{RouteId, Transport};

/// The payload of the unwind that ends an aborted worker. Not a panic: the hook never runs for it.
pub(super) struct Aborted;

/// The payload of the unwind that ends a worker stuck at a barrier: its result is Deadlock (spec.md 5).
pub(super) struct Stuck;

/// End this worker if the run aborted.
fn live(t: &Transport) {
    if t.aborted() {
        resume_unwind(Box::new(Aborted));
    }
}

/// The worker endpoint, and the route table, from one borrow of the process state: the two are
/// disjoint fields, so both can be used at once without aliasing the state twice.
fn parts() -> (&'static mut Transport, &'static [Entry]) {
    let l = local();
    let t = l.endpoint.as_mut().expect("trame: a worker operation outside a worker");
    live(t);
    (t, &l.entries)
}

fn route(entries: &[Entry], t: &Transport, to: WorkerId, ty: TypeId, name: &str) -> RouteId {
    let to_rank = rank_of(to, t.lay().workers);
    plan::find(entries, t.me(), to_rank, ty)
        .unwrap_or_else(|| panic!("trame: worker {} has no route of {name} to rank {to_rank}", t.me()))
}

pub(super) fn send(to: WorkerId, ty: TypeId, name: &str, bytes: &[u8]) -> Result<(), Full> {
    let (t, entries) = parts();
    let id = route(entries, t, to, ty, name);
    match t.send(id, bytes) {
        Ok(Answer::Ok) => Ok(()),
        Ok(Answer::Full) => Err(Full),
        Err(e) => panic!("trame: worker send of {name}: {e}"),
    }
}

pub(super) fn recv(ty: TypeId, out: &mut [u8]) -> bool {
    let l = local();
    let t = l.endpoint.as_mut().expect("trame: a worker operation outside a worker");
    live(t);
    let owned = plan::owned(&l.inbox, ty);
    match t.recv(owned, out) {
        Ok(Received::Message(_)) => true,
        Ok(Received::Empty) => false,
        Err(e) => panic!("trame: worker recv: {e}"),
    }
}

pub(super) fn offer(ty: TypeId) -> bool {
    let l = local();
    let t = l.endpoint.as_mut().expect("trame: a worker operation outside a worker");
    live(t);
    let owned = plan::owned(&l.inbox, ty);
    match t.offer(owned) {
        Ok(answer) => answer,
        Err(e) => panic!("trame: worker offer: {e}"),
    }
}

/// The index of this worker's route to `to` with `ty` among its outgoing routes (plan.rs `outgoing`).
pub(super) fn route_index(to: WorkerId, ty: TypeId) -> Option<usize> {
    let (t, entries) = parts();
    let to_rank = rank_of(to, t.lay().workers);
    plan::outgoing(entries, t.me()).position(|e| e.to == to_rank && e.ty == ty)
}

/// Whether the outgoing route at index `route` has room, as `route_index` numbers it.
pub(super) fn room(route: usize) -> bool {
    let (t, entries) = parts();
    let me = t.me();
    let id = plan::outgoing(entries, me)
        .nth(route)
        .map(|e| e.id)
        .unwrap_or_else(|| panic!("trame: worker {me} has no outgoing route {route} for this room check"));
    match t.room(id) {
        Ok(answer) => answer,
        Err(e) => panic!("trame: worker room: {e}"),
    }
}

/// The worker's epoch, widened to the surface's u64. Every value it returns is a u32 read from the window.
pub(super) fn epoch() -> u64 {
    u64::from(parts().0.epoch())
}

/// Sleep on the epoch `epoch` came from (see `epoch`). The narrowing is exact: the value was a u32.
pub(super) fn idle(until: Option<u64>, epoch: u64) {
    parts().0.sleep(epoch as u32, until);
}

/// Wait at the barrier, then return the host's publication. A worker stuck at it unwinds as `Stuck`,
/// and an aborted run as `Aborted`; run_worker (imp.rs) records each.
pub(super) fn barrier() -> Arc<[u8]> {
    let (t, _) = parts();
    match t.barrier() {
        Ok(bytes) => bytes,
        Err(Error::Deadlock) => resume_unwind(Box::new(Stuck)),
        Err(Error::Aborted) => resume_unwind(Box::new(Aborted)),
        Err(e) => panic!("trame: worker barrier: {e}"),
    }
}

pub(super) fn now() -> (ClockId, u64) {
    let (t, _) = parts();
    // The launch's start time names the clock: one clock for the run, shared by all its ranks.
    (ClockId::new(t.start() as u32), t.elapsed())
}

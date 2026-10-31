// The worker's side: every operation acts for this process's worker endpoint (state.rs). Each one
// first checks for an abort. An aborted run ends this worker at its next effect (spec.md 5, Abort):
// the check unwinds with `Aborted`, which the worker's catch_unwind (imp.rs) records as its own result.
//
// A worker rank runs several agents (threads), and the endpoint is one per rank and not `Sync`
// (state.rs). So every operation holds `XPORT`, the rank's transport lock, for its whole duration; steps
// still compute in parallel, only their effects are serialised. `idle` holds it through the futex
// sleep: an agent idles only when no step of its worker runs and none can start, so another agent that
// needs the lock meanwhile has nothing to do that the same epoch change would not also wake.

extern crate std;

use core::any::TypeId;
use std::boxed::Box;
use std::panic::resume_unwind;

use crate::io::Full;
use crate::time::ClockId;
use crate::topology::WorkerId;
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::plan::{self, Entry, rank_of};
use super::state::local;
use super::types::{Answer, Received};
use super::{RouteId, Transport};

/// The rank's transport lock (see the module comment).
static XPORT: Mutex<()> = Mutex::new(());

fn xport() -> MutexGuard<'static, ()> {
    XPORT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The payload of the unwind that ends an aborted worker. Not a panic: the hook never runs for it.
pub(super) struct Aborted;


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
    let _x = xport();
    let (t, entries) = parts();
    let id = route(entries, t, to, ty, name);
    match t.send(id, bytes) {
        Ok(Answer::Ok) => Ok(()),
        Ok(Answer::Full) => Err(Full),
        Err(e) => panic!("trame: worker send of {name}: {e}"),
    }
}

pub(super) fn recv(ty: TypeId, out: &mut [u8]) -> bool {
    let _x = xport();
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
    let _x = xport();
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
    let _x = xport();
    let (t, entries) = parts();
    let to_rank = rank_of(to, t.lay().workers);
    plan::outgoing(entries, t.me()).position(|e| e.to == to_rank && e.ty == ty)
}

/// Whether the outgoing route at index `route` has room, as `route_index` numbers it.
pub(super) fn room(route: usize) -> bool {
    let _x = xport();
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
    let _x = xport();
    u64::from(parts().0.epoch())
}

/// Sleep on the epoch `epoch` came from (see `epoch`). The narrowing is exact: the value was a u32.
pub(super) fn idle(until: Option<u64>, epoch: u64) {
    let _x = xport();
    parts().0.sleep(epoch as u32, until);
}

/// Run `f` on this rank's endpoint under the transport lock (imp.rs: results and aborts).
pub(super) fn with_endpoint<R>(f: impl FnOnce(&Transport) -> R) -> R {
    let _x = xport();
    let l = local();
    f(l.endpoint.as_ref().expect("trame: a worker operation outside a worker"))
}

/// Whether the leader stopped the run.
pub(super) fn stop_requested() -> bool {
    let _x = xport();
    parts().0.stop_requested()
}

/// This worker's rank.
pub(super) fn me() -> u32 {
    let _x = xport();
    parts().0.me()
}

pub(super) fn now() -> (ClockId, u64) {
    let _x = xport();
    let (t, _) = parts();
    // The clock is named by the node: each node has its own CLOCK_MONOTONIC start (NODES.md).
    (ClockId::new(t.nodes().me + 1), t.elapsed())
}

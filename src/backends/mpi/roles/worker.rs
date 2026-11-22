// The worker's side: every operation acts for this process's worker endpoint (state.rs). Each one
// first checks for an abort. An aborted run ends this worker at its next effect (INTERFACE.md 5, Abort):
// the check unwinds with `Aborted`, which the worker's catch_unwind (roles/runner.rs) records as its own result.
//
// A worker rank runs several agents (threads), and the endpoint is one per rank and not `Sync`
// (state.rs). So every operation holds `XPORT`, the rank's transport lock, for its whole duration; steps
// still compute in parallel, only their effects are serialised. `idle` holds it through the futex
// sleep: an agent idles only when no step of its worker runs and none can start, so another agent that
// needs the lock meanwhile has nothing to do that the same epoch change would not also wake. A worker
// with a lane (transport/cross/) cannot sleep: a frame from another host moves no futex. Its idle agents poll
// the lanes instead, taking the lock for each poll only, until the epoch moves.
//
// Deadlock (INTERFACE.md 5). The scheduler tells its turns (`mark_*`, under its own lock) into the
// worker's RUN, ACTS and IDLE words, which the leader's detector reads (detect.rs). When the detector
// shows every worker of the host stuck, the leader sets DEADLOCK and wakes every agent; each unwinds at
// its next round with `Deadlocked`, and the worker's result is OUT_DEADLOCK (roles/runner.rs).

extern crate std;

use core::any::TypeId;
use std::boxed::Box;
use std::panic::resume_unwind;

use crate::io::Full;
use crate::time::ClockId;
use crate::topology::End;
use std::sync::atomic::Ordering::{Acquire, SeqCst};
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use crate::backends::mpi::memory::layout::{ABORT_BIT, DEADLOCK, DONE, K_ACTS, K_EDIT, K_HELD, K_IDLE, K_RUN, STOP};

use crate::backends::mpi::setup::plan::{self, Entry, rank_of};
use crate::backends::mpi::setup::state::local;
use crate::backends::mpi::types::{Answer, Received};
use crate::backends::mpi::{RouteId, Transport};

/// The rank's transport lock (see the module comment).
static XPORT: Mutex<()> = Mutex::new(());

fn xport() -> MutexGuard<'static, ()> {
    XPORT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The payload of the unwind that ends an aborted worker. Not a panic: the hook never runs for it.
pub(crate) struct Aborted;

/// The payload of the unwind that ends a worker the leader showed stuck (detect.rs). Not a panic.
pub(crate) struct Deadlocked;

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

fn route(entries: &[Entry], t: &Transport, to: End, ty: TypeId, name: &str) -> RouteId {
    plan::find(entries, plan::here(t.me()), rank_of(to, t.lay().workers), ty)
        .unwrap_or_else(|| panic!("trame: worker {} has no route of {name} to {to:?}", t.me()))
}

pub(crate) fn send(to: End, ty: TypeId, name: &str, bytes: &[u8]) -> Result<(), Full> {
    let _x = xport();
    let (t, entries) = parts();
    let id = route(entries, t, to, ty, name);
    match t.send(id, bytes) {
        Ok(Answer::Ok) => Ok(()),
        Ok(Answer::Full) => Err(Full),
        Err(e) => panic!("trame: worker send of {name}: {e}"),
    }
}

pub(crate) fn recv(ty: TypeId, out: &mut [u8]) -> bool {
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

pub(crate) fn offer(ty: TypeId) -> bool {
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
pub(crate) fn route_index(to: End, ty: TypeId) -> Option<usize> {
    let _x = xport();
    let (t, entries) = parts();
    let to = rank_of(to, t.lay().workers);
    plan::outgoing(entries, t.me()).position(|e| e.to == to && e.ty == ty)
}

/// Whether the outgoing route at index `route` has room, as `route_index` numbers it.
pub(crate) fn room(route: usize) -> bool {
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

/// The window words an agent uses without the transport lock, as raw addresses: its epoch, STOP,
/// DONE, its EDIT and HELD lines, and the leader's epoch and sleep count (to wake it). Set once by
/// `fast_init` before any agent starts; the window lives until the process exits (state.rs), and these
/// are atomic words. The edit handshake must not take the lock: an idle agent holds it through its
/// futex sleep, and the agent that answers the leader may be another one.
static FAST: OnceLock<Fast> = OnceLock::new();

struct Fast {
    epoch: usize,
    stop: usize,
    done: usize,
    edit: usize,
    held: usize,
    leader_epoch: usize,
    leader_sleep: usize,
    deadlock: usize,
    run: usize,
    acts: usize,
    idle: usize,
}

/// Record this worker's round words (roles/runner.rs, before the agents start).
pub(crate) fn fast_init() {
    let _x = xport();
    let (t, _) = parts();
    let at = |off: usize| t.win().ptr(off, 8) as usize;
    let (epoch, _) = t.words(t.me());
    let (leader_epoch, leader_sleep) = t.words(t.lay().workers);
    let _ = FAST.set(Fast {
        epoch: at(epoch),
        stop: at(STOP),
        done: at(DONE),
        edit: at(t.lay().per_worker(K_EDIT, t.me())),
        held: at(t.lay().per_worker(K_HELD, t.me())),
        leader_epoch: at(leader_epoch),
        leader_sleep: at(leader_sleep),
        deadlock: at(DEADLOCK),
        run: at(t.lay().per_worker(K_RUN, t.me())),
        acts: at(t.lay().per_worker(K_ACTS, t.me())),
        idle: at(t.lay().per_worker(K_IDLE, t.me())),
    });
}

fn fast() -> (&'static AtomicU32, &'static AtomicU64, &'static AtomicU32) {
    let f = FAST.get().expect("trame: fast_init runs before the agents");
    // SAFETY: addresses of aligned words inside the window, which outlives every agent (state.rs).
    unsafe { (&*(f.epoch as *const AtomicU32), &*(f.stop as *const AtomicU64), &*(f.done as *const AtomicU32)) }
}

fn word64(addr: usize) -> &'static AtomicU64 {
    // SAFETY: as `fast`.
    unsafe { &*(addr as *const AtomicU64) }
}

/// A step starts (Backend::worker_mark_start): under the scheduler's lock, so without XPORT.
pub(crate) fn mark_start() {
    let f = FAST.get().expect("trame: fast_init runs before the agents");
    word64(f.run).fetch_add(1, SeqCst);
    word64(f.acts).fetch_add(1, SeqCst);
    word64(f.idle).store(0, SeqCst);
}

/// A step ends.
pub(crate) fn mark_end() {
    let f = FAST.get().expect("trame: fast_init runs before the agents");
    word64(f.run).fetch_sub(1, SeqCst);
    word64(f.idle).store(0, SeqCst);
}

/// An agent idles at `epoch`: IDLE says so (layout.rs IDLE).
pub(crate) fn mark_quiet(epoch: u64) {
    let f = FAST.get().expect("trame: fast_init runs before the agents");
    word64(f.idle).store(epoch + 1, SeqCst);
}

/// The fields the leader asks this worker to hold, 0 if none (layout.rs EDIT). Without the lock.
pub(crate) fn edit_request() -> u64 {
    let f = FAST.get().expect("trame: fast_init runs before the agents");
    word64(f.edit).load(SeqCst)
}

/// Answer the leader's edit request (layout.rs HELD), and wake it: the same protocol as
/// `Transport::bump` on the leader's epoch (epoch.rs), on raw addresses, without the lock.
pub(crate) fn edit_held(held: bool) {
    let f = FAST.get().expect("trame: fast_init runs before the agents");
    word64(f.held).store(u64::from(held), SeqCst);
    // SAFETY: the leader's epoch is an aligned u32 of the window (as `fast`).
    let e = unsafe { &*(f.leader_epoch as *const AtomicU32) };
    e.fetch_add(1, SeqCst);
    if word64(f.leader_sleep).load(SeqCst) != 0 {
        crate::backends::mpi::waiting::futex::wake_addr(f.leader_epoch);
    }
}

/// The worker's epoch, widened to the surface's u64. Read without the transport lock (FAST); an
/// aborted run unwinds here, as at every effect, so an agent whose steps make no effects still ends.
pub(crate) fn epoch() -> u64 {
    let (e, _, d) = fast();
    if d.load(SeqCst) & ABORT_BIT != 0 {
        resume_unwind(Box::new(Aborted));
    }
    let f = FAST.get().expect("trame: fast_init runs before the agents");
    if word64(f.deadlock).load(SeqCst) != 0 {
        resume_unwind(Box::new(Deadlocked));
    }
    u64::from(e.load(SeqCst))
}

/// Sleep on the epoch `epoch` came from (see `epoch`). The narrowing is exact: the value was a u32.
pub(crate) fn idle(epoch: u64) {
    let x = xport();
    let t = parts().0;
    if !t.has_lanes() {
        t.sleep(epoch as u32);
        return;
    }
    drop(x);
    let (e, _, _) = fast();
    loop {
        let x = xport();
        let t = parts().0;
        t.poll_lanes();
        drop(x);
        if e.load(SeqCst) != epoch as u32 {
            return;
        }
        std::hint::spin_loop();
    }
}

/// Run `f` on this rank's endpoint under the transport lock (roles/runner.rs: results and aborts).
pub(crate) fn with_endpoint<R>(f: impl FnOnce(&Transport) -> R) -> R {
    let _x = xport();
    let l = local();
    f(l.endpoint.as_ref().expect("trame: a worker operation outside a worker"))
}

/// Whether the leader stopped the run. Without the transport lock (FAST).
pub(crate) fn stop_requested() -> bool {
    fast().1.load(Acquire) != 0
}

/// This worker's rank.
pub(crate) fn me() -> u32 {
    let _x = xport();
    parts().0.me()
}

pub(crate) fn now() -> (ClockId, u64) {
    let _x = xport();
    let (t, _) = parts();
    // The clock is named by the host: each host has its own CLOCK_MONOTONIC start (hosts.rs).
    (ClockId::new(u64::from(crate::backends::mpi::transport::hosts::host().id) + 1), t.elapsed())
}

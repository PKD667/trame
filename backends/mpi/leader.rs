// The leader's side: the Run the backend hands the host, and its sends, takes and join. The leader is
// rank `workers`; its endpoint is a Transport, which needs `&mut` for a send or a take. The trait
// gives `&Run`, so the endpoint sits in an UnsafeCell. The leader's calls run on the rank's main
// thread one at a time, and none calls another, so no two `&mut` to it are ever alive together.

extern crate std;

use core::any::TypeId;
use core::cell::UnsafeCell;
use std::vec::Vec;

use crate::io::Full;
use crate::run::Outcome;
use crate::topology::WorkerId;

use super::layout::{OUT_DEADLOCK, OUT_RETURNED};
use super::plan::{self, Entry, rank_of};
use super::state::{finalize, local};
use super::types::{Answer, Received};
use super::{RouteId, Transport};

impl Transport {
    /// The leader's endpoint is dropped by a panic before `join`. A panic ends the run (spec.md 5, Abort): the
    /// workers unwind, and END stops the relays, without which the process hangs at exit. The panic hook has
    /// printed the message. END is set even when an abort is already recorded: one may have arrived from a
    /// worker before the leader's panic, and then nothing else ends the run (proofs/mpi, `MCAbortEarly`).
    /// `end_run` may run twice: a publication object already gone is not an error. After `MPI_Finalize`
    /// (`fail_if_aborted` ended the run) there is nothing left to do.
    pub(super) fn leader_panicked(&self) {
        let mut done: i32 = 0;
        // SAFETY: MPI_Finalized takes one out-pointer and may be called at any time.
        unsafe { mpi::ffi::MPI_Finalized(&mut done) };
        if done != 0 {
            return;
        }
        if !self.aborted() {
            self.record_abort(self.me(), b"the leader panicked before join");
        }
        self.end_run();
    }
}

/// A launched run, as the leader holds it: the leader's endpoint.
pub struct Leader(UnsafeCell<Transport>);

impl Leader {
    pub(super) fn new(t: Transport) -> Leader {
        Leader(UnsafeCell::new(t))
    }

    /// The endpoint. Each call is one leader operation, which ends before the next begins.
    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> &mut Transport {
        // SAFETY: see the module comment. The leader is one thread, and no operation holds this
        // reference across another operation.
        unsafe { &mut *self.0.get() }
    }
}

/// The route the leader sends on to `to` with message type `ty`. Panics when there is none.
fn route(entries: &[Entry], t: &Transport, to: WorkerId, ty: TypeId, name: &str) -> RouteId {
    let to_rank = rank_of(to, t.lay().workers);
    plan::find(entries, t.me(), to_rank, ty)
        .unwrap_or_else(|| panic!("trame: the leader has no route of {name} to rank {to_rank}"))
}

pub(super) fn send(run: &Leader, to: WorkerId, ty: TypeId, name: &str, bytes: &[u8]) -> Result<(), Full> {
    let t = run.get();
    t.fail_if_aborted();
    let id = route(&local().entries, t, to, ty, name);
    match t.send(id, bytes) {
        Ok(Answer::Ok) => Ok(()),
        Ok(Answer::Full) => Err(Full),
        Err(e) => panic!("trame: leader send of {name}: {e}"),
    }
}

pub(super) fn recv(run: &Leader, ty: TypeId, out: &mut [u8]) -> bool {
    let t = run.get();
    t.fail_if_aborted();
    let owned = plan::owned(&local().inbox, ty);
    match t.recv(owned, out) {
        Ok(Received::Message(_)) => true,
        Ok(Received::Empty) => false,
        Err(e) => panic!("trame: leader recv: {e}"),
    }
}

/// Deposit a publication (gate.rs `publish`). Never waits, so the leader's host code is not held up by workers.
pub(super) fn publish(run: &Leader, bytes: &[u8]) {
    run.get().fail_if_aborted();
    run.get().publish(bytes)
}

/// Block until the leader's epoch differs from `seen`, and answer the new value (epoch.rs).
pub(super) fn wait(run: &Leader, seen: u64) -> u64 {
    run.get().fail_if_aborted();
    let e = run.get().leader_wait(seen);
    run.get().fail_if_aborted();
    e
}

/// Wait for every worker's result (see `join` in results.rs), in declaration order. Then finalize:
/// every outcome is read, so no result can depend on this rank's exit. An aborted run panics instead.
pub(super) fn join(run: Leader) -> Vec<Outcome> {
    let words = run.get().join();
    run.get().fail_if_aborted();
    let out = words
        .into_iter()
        .map(|w| match w {
            OUT_RETURNED => Outcome::Returned,
            OUT_DEADLOCK => Outcome::Deadlock,
            other => panic!("trame: unknown outcome word {other}"),
        })
        .collect();
    run.get().end_run();
    finalize();
    out
}

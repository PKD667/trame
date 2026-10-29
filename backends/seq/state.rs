//! State of one sequential run: the baton, the phases of the workers, the routes and the
//! publication. Every function here runs with the Mutex held; ops.rs does the locking.

extern crate std;

use super::limit;
use crate::io::Full;
use crate::run::Outcome;
use crate::topology::WorkerId;
use std::any::TypeId;
use std::string::String;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use std::vec::Vec;

/// Where a worker stands. Only the holder of the baton runs. `Blocked` waits at the barrier, and
/// `Done` has a result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Ready,
    Blocked,
    /// Idle: waiting for a change of `acts` (or the deadline, in nanoseconds since `epoch`) before it
    /// can run again. The first value is `acts` when it went idle, so `join` can tell whether a
    /// change happened since. A deadline means the worker moves when time passes, so it is not stuck.
    Idle(u64, Option<u64>),
    Done(Outcome),
}

/// A route with its frames. `from` sends, `to` receives; `ty` is its tag. The frames live in
/// `buf`, a ring of `cap` slots of `size` bytes, allocated once at launch: a send copies into a
/// slot and a receive copies out of one, so neither allocates. The oldest frame is at `head`.
pub(super) struct Route {
    pub from: WorkerId,
    pub to: WorkerId,
    pub ty: TypeId,
    pub cap: usize,
    pub size: usize,
    pub lossy: bool,
    pub buf: Vec<u8>,
    pub head: usize,
    pub len: usize,
    /// The number of the take that last took a frame from this route; 0 before the first.
    pub taken: u64,
}

impl Route {
    fn slot(&self, at: usize) -> core::ops::Range<usize> {
        at * self.size..(at + 1) * self.size
    }
}

pub(super) struct State {
    pub phase: Vec<Phase>,
    /// The worker that holds the baton, or `None` when no worker can run.
    pub turn: Option<usize>,
    pub aborted: bool,
    /// The first panic of the run: the worker's index and its message. Set with `aborted` by a panic;
    /// `None` when the run aborted without one (a worker thread that could not start).
    pub panic: Option<(usize, String)>,
    pub routes: Vec<Route>,
    /// The takes so far, numbering them for `Route::taken`.
    pub takes: u64,
    /// The publication of the host, shared with the guards workers hold (see io::Publication).
    pub publication: Arc<[u8]>,
    /// A publication the leader made, installed when the barrier next fires.
    pub pending: Option<Arc<[u8]>>,
    pub epoch: Instant,
    /// Counts every effect a worker finished and every leader send or publication. An idle worker
    /// waits for this to change (see `worker_idle` in ops.rs).
    pub acts: u64,
    /// Scheduler steps taken: one per hand-off of the baton (`advance`).
    pub steps: u64,
    /// TRAME_SEQ_STEP_LIMIT, read at launch (limit.rs).
    pub limit: Option<u64>,
}

pub(super) struct Shared {
    pub state: Mutex<State>,
    /// Signalled on every change of phase or baton.
    pub changed: Condvar,
}

/// The unwinding payload of a worker that must stop without a result of its own, because the run
/// ended or it was deadlocked. It is not a panic, so the panic hook does not print it.
pub(super) struct Abandoned;

impl Shared {
    /// Lock the state. A panic in a worker poisons the Mutex; the state is still consistent
    /// enough to end the run, so the poison is ignored and the run is aborted instead.
    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn park<'a>(&self, guard: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.changed.wait(guard).unwrap_or_else(PoisonError::into_inner)
    }

    /// Park until a change, or for at most `dur`. A spurious or timed-out wake-up is harmless: the
    /// caller checks its condition again.
    pub fn park_timeout<'a>(&self, guard: MutexGuard<'a, State>, dur: Duration) -> MutexGuard<'a, State> {
        self.changed.wait_timeout(guard, dur).unwrap_or_else(PoisonError::into_inner).0
    }
}

impl State {
    /// The first worker after `from`, in declaration order and cyclically, that can move.
    fn next_ready(&self, from: usize) -> Option<usize> {
        let n = self.phase.len();
        (1..=n).map(|k| (from + k) % n).find(|&i| self.phase[i] == Phase::Ready)
    }

    /// Hand the baton on from `from`. If no worker can move, the run is stuck. The barrier fires
    /// as soon as every worker is at it, so a stuck run has a worker that returned, and the workers
    /// at the barrier can never be released: they get `Deadlock`.
    pub fn advance(&mut self, from: usize) {
        self.steps += 1;
        if let Some(n) = self.limit.filter(|&n| self.steps > n) {
            limit::exceeded(n);
        }
        // An idle worker can still be woken by the leader, so the run is not stuck while one exists.
        if !self.phase.iter().any(|p| matches!(p, Phase::Ready | Phase::Idle(..))) {
            for p in self.phase.iter_mut().filter(|p| **p == Phase::Blocked) {
                *p = Phase::Done(Outcome::Deadlock);
            }
        }
        self.turn = self.next_ready(from);
    }

    /// Every worker is at the barrier: install the pending publication, release everyone.
    pub fn fire(&mut self) {
        if let Some(bytes) = self.pending.take() {
            self.publication = bytes;
        }
        for p in self.phase.iter_mut() {
            *p = Phase::Ready;
        }
    }

    /// The run aborted: no baton is held, and every agent unwinds at its next effect. The first panic
    /// is kept; a later one does not replace it. The leader's next call panics with it (ops.rs).
    pub fn abort_all(&mut self, panic: Option<(usize, String)>) {
        self.aborted = true;
        self.turn = None;
        if self.panic.is_none() {
            self.panic = panic;
        }
    }

    /// Whether worker `i` must unwind rather than run on: the run was aborted, or it has a result.
    pub fn must_stop(&self, i: usize) -> bool {
        self.aborted || matches!(self.phase[i], Phase::Done(_))
    }

    /// The index of the route from `from` to `to` with tag `ty`, if the topology has one.
    pub fn route(&self, from: WorkerId, to: WorkerId, ty: TypeId) -> Option<usize> {
        self.routes.iter().position(|r| r.from == from && r.to == to && r.ty == ty)
    }

    /// Append a frame to route `i` (spec.md Send, Full, Overwrite). A lossy route at capacity
    /// overwrites its oldest frame; any other route at capacity answers `Full`.
    pub fn push(&mut self, i: usize, bytes: &[u8]) -> Result<(), Full> {
        let r = &mut self.routes[i];
        let at = if r.len < r.cap {
            r.len += 1;
            (r.head + r.len - 1) % r.cap
        } else if r.lossy {
            let at = r.head;
            r.head = (r.head + 1) % r.cap;
            at
        } else {
            return Err(Full);
        };
        let range = r.slot(at);
        r.buf[range].copy_from_slice(bytes);
        Ok(())
    }

    /// Copy the oldest frame of a route to `to` with tag `ty` into `out`, and drop it from the route.
    /// Of the routes that have a frame, the one taken least recently is taken: a route that keeps a
    /// frame is then taken within one round of its siblings, whatever is taken in between (spec.md 6,
    /// Progress). A cursor shared by every route and tag is not enough: a periodic mix of takes can
    /// bring it back to the same route each time. Answers `false` when no route has a frame.
    pub fn take(&mut self, to: WorkerId, ty: TypeId, out: &mut [u8]) -> bool {
        let next = self.takes + 1;
        let Some(r) = self.routes.iter_mut().filter(|r| r.to == to && r.ty == ty && r.len > 0).min_by_key(|r| r.taken)
        else {
            return false;
        };
        let range = r.slot(r.head);
        out.copy_from_slice(&r.buf[range]);
        r.head = (r.head + 1) % r.cap;
        r.len -= 1;
        r.taken = next;
        self.takes = next;
        true
    }
}

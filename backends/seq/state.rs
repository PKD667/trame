//! State of one sequential run: the baton, the phases of the workers, the routes, the workers'
//! schedulers and the pool. Every function here runs with the Mutex held; ops.rs does the locking.

extern crate std;

use super::limit;
use crate::local_routes::Routes;
use crate::backend::PoolRef;
use crate::run::Outcome;
use std::string::String;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use std::vec::Vec;

/// Where a worker stands. Only the holder of the baton runs. `Done` has a result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Ready,
    /// Idle: waiting for a change of `acts` (or the deadline, in nanoseconds since `epoch`) before it
    /// can run again. The first value is `acts` when it went idle, so `join` can tell whether a
    /// change happened since. A deadline means the worker moves when time passes, so it is not stuck.
    Idle(u64, Option<u64>),
    Done(Outcome),
}

pub(super) struct State {
    pub phase: Vec<Phase>,
    /// The worker that holds the baton, or `None` when no worker can run.
    pub turn: Option<usize>,
    pub aborted: bool,
    /// The first panic of the run: the worker's index and its message. Set with `aborted` by a panic;
    /// `None` when the run aborted without one (a worker thread that could not start).
    pub panic: Option<(usize, String)>,
    pub routes: Routes,
    /// The host's pool: the current version. A step pins it by a clone at its start (backend.rs, P1).
    pub pool: Option<PoolRef>,
    pub epoch: Instant,
    /// Counts every effect a worker finished and every leader act (send, take, pool, edit, stop). An idle worker
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

    /// Hand the baton on from `from`, to the next worker that can move, if any.
    pub fn advance(&mut self, from: usize) {
        self.steps += 1;
        if let Some(n) = self.limit.filter(|&n| self.steps > n) {
            limit::exceeded(n);
        }
        self.turn = self.next_ready(from);
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
}

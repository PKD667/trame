//! The leader's side of a run: spec.md sections 5 and 6. The leader is part of the environment, so
//! its operations are not rules of T; the backend records what it does.

use crate::backend::{Backend, Selected};
use crate::io::{Full, Message, read};
use crate::topology::{Worker, WorkerId};
use crate::worker::State;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::{Any, TypeId, type_name};
use core::cell::Cell;
use core::sync::atomic::{AtomicBool, Ordering};

/// A launched run, as the leader holds it. Dropping it does not stop the workers.
pub struct Run {
    inner: <Selected as Backend>::Run,
    /// The leader's counter as of its last `wait` (see `wait`).
    seen: Cell<u64>,
    pool_type: TypeId,
    workers: usize,
    /// Set by `join`: the workers' states are the leader's again (topology.rs, `Worker::take`).
    joined: Arc<AtomicBool>,
}

impl Run {
    pub(crate) fn new(inner: <Selected as Backend>::Run, pool_type: TypeId, workers: usize, joined: Arc<AtomicBool>) -> Self {
        Run { inner, seen: Cell::new(0), pool_type, workers, joined }
    }

    /// Block until something changed for the leader since its last `wait`: a frame arrived for the
    /// leader, or a worker got a result, or the run aborted. Each call is caused by such a change, so a
    /// leader that checks `recv` and then waits does not poll. Pass it the result of a recv that found
    /// nothing, and check again after it returns.
    pub fn wait(&self) {
        self.seen.set(Selected::leader_wait(&self.inner, self.seen.get()));
    }

    /// Send `msg` from the leader to `to`, on the route of type `M`. Panics when there is no such
    /// route. `Full` only on a non-lossy route at capacity.
    pub fn send<M: Message>(&self, to: WorkerId, msg: M) -> Result<(), Full> {
        Selected::leader_send(&self.inner, to, TypeId::of::<M>(), type_name::<M>(), crate::__bytemuck::bytes_of(&msg))
    }

    /// The next message of type `M` addressed to the leader, or `None`.
    pub fn recv<M: Message>(&self) -> Option<M> {
        read(|out| Selected::leader_recv(&self.inner, TypeId::of::<M>(), out))
    }

    /// Replace the host's pool. A step that starts after this returns reads `pool`; a step already
    /// running keeps the version it started with, which is freed once no step reads it. Panics when
    /// `P` is not the type given to `Topology::pool`.
    pub fn publish<P: Send + Sync + 'static>(&self, pool: P) {
        assert!(
            TypeId::of::<P>() == self.pool_type,
            "trame: Run::publish of a {}, but the topology's pool has another type",
            type_name::<P>()
        );
        Selected::leader_pool(&self.inner, Arc::new(pool))
    }

    /// Edit a worker's state: `f` runs once no step of the worker runs, and no step starts until it
    /// returns. It may allocate and resize. Frames queued for the worker are kept, and every step of
    /// the worker is woken afterwards. Answers `false`, without running `f`, when the worker already
    /// has a result.
    pub fn edit<S: State>(&self, w: &Worker<S>, f: impl FnOnce(&mut S)) -> bool {
        let index = w.id().0 as usize;
        assert!(index < self.workers, "trame: Run::edit of a worker of another topology");
        let mut f = Some(f);
        let core = w.core();
        Selected::leader_edit(&self.inner, index, &mut || {
            let f = f.take().expect("trame: an edit runs its function once");
            // SAFETY: the backend runs this while no step of the worker runs (backend.rs, S4).
            unsafe { core.edit_with(f) }
        })
    }

    /// Stop the run: no ordinary step starts again; each worker runs its stop step, when it has one,
    /// alone until it is done. Returns at once; keep receiving until `join`, since a stop step may
    /// send.
    pub fn stop(&self) {
        Selected::leader_stop(&self.inner)
    }

    /// Join, then replay every worker's trace against the model (sched.rs `check`); `exact` also
    /// checks each choice. Answers the outcomes and the number of events replayed, or the first
    /// violation with the worker's index. Needs `Topology::__trace`.
    #[doc(hidden)]
    pub fn __join_checked(self, exact: bool) -> (Vec<Outcome>, Result<usize, (usize, String)>) {
        let workers = self.workers;
        let (out, traces) = Selected::leader_join_traced(self.inner, workers);
        self.joined.store(true, Ordering::Release);
        let mut events = 0;
        for (w, t) in traces.into_iter().enumerate() {
            let (model, trace) = t.expect("trame: the launch did not ask for traces");
            events += trace.len();
            let end = match crate::sched::check(model, &trace, exact) {
                Ok(end) => end,
                Err(v) => return (out, Err((w, alloc::format!("{v:?}")))),
            };
            // The outcome must be the model's: a result it reached, or a deadlock in which no step
            // could start even if every deadline passed (else a wake-up was lost, backend.rs S3).
            let agrees = match out[w] {
                Outcome::Returned => end.answer() == Some(crate::sched::Answer::Returned),
                Outcome::Stopped => end.answer() == Some(crate::sched::Answer::Stopped),
                Outcome::Failed => matches!(end.answer(), Some(crate::sched::Answer::Failed(_))),
                Outcome::Deadlock => {
                    let late = crate::sched::Now { until: u64::MAX, ..Default::default() };
                    end.answer().is_none() && (0..crate::sched::MAX_STEPS).all(|i| !end.may_start(i, &late))
                }
            };
            if !agrees {
                let msg = alloc::format!("outcome {:?} disagrees with the model's final state {:?}", out[w], end.answer());
                return (out, Err((w, msg)));
            }
        }
        (out, Ok(events))
    }

    /// Wait until every worker has a result, and return them in declaration order. A panic in a
    /// worker aborts the run, and then this panics too (see `wait`).
    pub fn join(self) -> Vec<Outcome> {
        let out = Selected::leader_join(self.inner);
        self.joined.store(true, Ordering::Release);
        out
    }
}

/// The result of one worker: spec.md 5, "Deadlock". A panic never has a result: it panics the leader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every step of the worker is done.
    Returned,
    /// The leader stopped the run, and the worker's stop step, if any, is done.
    Stopped,
    /// A step failed; `Worker::error` has its error.
    Failed,
    /// The run was stuck: no worker could move, and the backend ended it.
    Deadlock,
}

/// The text of a panic, as the leader reports it: the payload's message, or `<non-string panic
/// payload>` when the payload is neither a `&str` nor a `String` (spec.md 5, Abort).
pub(crate) fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

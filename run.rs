//! The leader's side of a run: spec.md sections 5 and 6. The leader is part of the environment, so
//! its operations are not rules of T; the backend records what it does.

use crate::backend::{Backend, Selected};
use crate::io::{Full, Message, read};
use crate::topology::{Handle, HostId, WorkerId};
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
    /// The pool's address, to check a `Run::edit` of it is of this run.
    pool: Option<*const ()>,
    workers: usize,
    /// Set by `join`: the workers' states are the leader's again (topology.rs, `Worker::take`).
    joined: Arc<AtomicBool>,
}

impl Run {
    pub(crate) fn new(inner: <Selected as Backend>::Run, pool: Option<*const ()>, workers: usize, joined: Arc<AtomicBool>) -> Self {
        Run { inner, seen: Cell::new(0), pool, workers, joined }
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
    pub fn send<M: Message>(&self, to: impl Into<WorkerId>, msg: M) -> Result<(), Full> {
        Selected::leader_send(&self.inner, to.into(), TypeId::of::<M>(), type_name::<M>(), crate::__bytemuck::bytes_of(&msg))
    }

    /// The next message of type `M` addressed to the leader, or `None`.
    pub fn recv<M: Message>(&self) -> Option<M> {
        read(|out| Selected::leader_recv(&self.inner, TypeId::of::<M>(), out))
    }

    /// Send `msg` to the leader of host `to`, on the peer channel, not a route (CONTRACT.md F6).
    /// `Full` when that channel is at capacity. Panics when `to` is not a host of the federation.
    pub fn leader_send<M: Message>(&self, to: HostId, msg: M) -> Result<(), Full> {
        Selected::peer_send(&self.inner, to.0, crate::io::tag::<M>(), crate::__bytemuck::bytes_of(&msg))
    }

    /// The next message of type `M` from a peer leader, with the host it came from, or `None`.
    pub fn leader_recv<M: Message>(&self) -> Option<(HostId, M)> {
        let mut from = 0;
        read(|out| Selected::peer_recv(&self.inner, crate::io::tag::<M>(), out).map(|h| from = h).is_some()).map(|m| (HostId(from), m))
    }

    /// Edit `h`, a worker's state or the pool: `f` runs once no step that uses it runs, and no such
    /// step starts until `f` returns. For a worker, that is every step of the worker; for the pool,
    /// every step that takes `pool: &P`, in every worker. `f` may allocate and resize. Frames are kept,
    /// and every step that uses `h` is woken afterwards. A worker asks for a pool edit by sending the
    /// leader a message. Answers `false`, without running `f`, when the worker already has a result;
    /// a pool edit answers `true`.
    pub fn edit<H: Handle>(&self, h: &H, f: impl FnOnce(&mut H::Value)) -> bool {
        let target = h.target(self);
        let mut f = Some(f);
        Selected::leader_edit(&self.inner, target, &mut || {
            let mut f = f.take();
            // SAFETY: the backend runs this holding target's fields (CONTRACT.md, S4, P2).
            // Arena mode: what the edit allocates lands in the domain (CONTRACT.md, M1).
            crate::domain::arena(|| unsafe { h.edit_with(&mut |v| (f.take().expect("trame: an edit runs its function once"))(v)) })
        })
    }

    /// Its topology's join flag, which that topology's handles share (topology.rs, `target`).
    pub(crate) fn joined(&self) -> &Arc<AtomicBool> {
        &self.joined
    }

    pub(crate) fn pool(&self) -> Option<*const ()> {
        self.pool
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

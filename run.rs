//! The leader's side of a run: spec.md sections 5 and 6. The leader is part of the environment, so
//! its operations are not rules of T; the backend records what it does.

use crate::backend::{Backend, Selected};
use crate::io::{Full, Message, read};
use crate::topology::WorkerId;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::any::{Any, TypeId, type_name};
use core::cell::Cell;

/// A launched run, as the leader holds it. Dropping it does not stop the workers.
pub struct Run {
    inner: <Selected as Backend>::Run,
    /// The leader's counter as of its last `wait` (see `wait`).
    seen: Cell<u64>,
}

impl Run {
    pub(crate) fn new(inner: <Selected as Backend>::Run) -> Self {
        Run { inner, seen: Cell::new(0) }
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

    /// Replace the host's publication. Workers see it at the next barrier that fires.
    pub fn publish(&self, bytes: &[u8]) {
        Selected::leader_publish(&self.inner, bytes)
    }

    /// Wait until every worker has a result, and return them in declaration order. A panic in a
    /// worker aborts the run, and then this panics too (see `wait`).
    pub fn join(self) -> Vec<Outcome> {
        Selected::leader_join(self.inner)
    }
}

/// The result of one worker: spec.md 5, "Deadlock". A panic never has a result: it panics the leader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The worker's `run` returned.
    Returned,
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

//! Stuckness of the seq backend (INTERFACE.md 5, Definition Deadlock), shared by `join` and the leader's
//! `wait` (ops.rs).

use super::state::{Phase, State};

impl State {
    /// Whether a worker moves now, without time passing: it is ready (it holds or waits for the
    /// baton), or it is idle and a change of `acts` since it went idle wakes it.
    pub(super) fn worker_moves_now(&self) -> bool {
        self.phase.iter().any(|p| match p {
            Phase::Ready => true,
            Phase::Idle(seen) => *seen != self.acts,
            _ => false,
        })
    }

    /// The leader, blocked in `wait`, can never return. `wait(seen)` returns when `acts` moves past
    /// `seen`, and only a worker can move it now: so it is stuck when no worker moves now. A frame already queued for the leader does not count: the leader saw it before `seen`.
    /// The seq backend has one leader (`Leader`), so no other leader can act while this one waits.
    pub(super) fn leader_stuck(&self) -> bool {
        !self.worker_moves_now()
    }
}

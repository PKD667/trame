//! Stuckness of the seq backend (spec.md 5, Definition Deadlock), shared by `join` and the leader's
//! `wait` (ops.rs).

use super::state::{Phase, State};

impl State {
    /// Whether a worker moves now, without time passing: it is ready (it holds or waits for the
    /// baton), or it is idle and a change of `acts` since it went idle wakes it.
    pub(super) fn worker_moves_now(&self) -> bool {
        self.phase.iter().any(|p| match p {
            Phase::Ready => true,
            Phase::Idle(seen, _) => *seen != self.acts,
            _ => false,
        })
    }

    /// The earliest deadline of an idle worker, in nanoseconds since `epoch`. Time moves such a worker.
    pub(super) fn earliest_deadline(&self) -> Option<u64> {
        self.phase
            .iter()
            .filter_map(|p| match p {
                Phase::Idle(_, Some(t)) => Some(*t),
                _ => None,
            })
            .min()
    }

    /// The leader, blocked in `wait`, can never return. `wait(seen)` returns when `acts` moves past
    /// `seen`, and only a worker can move it now: so it is stuck when no worker moves now or after time
    /// passes. A frame already queued for the leader does not count: the leader saw it before `seen`.
    /// The seq backend has one leader (`LEADER`), so no other leader can act while this one waits.
    pub(super) fn leader_stuck(&self) -> bool {
        !self.worker_moves_now() && self.earliest_deadline().is_none()
    }
}

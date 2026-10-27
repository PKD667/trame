//! The seam between the surface and the selected backend. The surface calls these functions and
//! nothing else in a backend. A backend implements `Backend` on its `Selected` type, so the
//! compiler checks every backend against these signatures.
//!
//! Worker operations act for the calling thread, which must be a worker. Leader operations act for
//! the `Run` they are given. Bytes cross this seam; typed messages are encoded in `io`.

use crate::io::Full;
use crate::launch::{Plan, Refused};
use crate::run::Outcome;
use crate::time::ClockId;
use crate::topology::WorkerId;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::TypeId;

pub(crate) trait Backend {
    /// The state of one launched run, owned by the leader.
    type Run;

    /// Check that every process of the launch built the same plan, before the local checks run. A
    /// backend with one process has nothing to agree on. A backend with several processes makes this
    /// a collective: every process calls it, in the same order, and every process gets the same answer.
    /// It runs before `check`, so no process refuses while a peer waits for it in that collective.
    fn agree(_plan: &Plan) -> Result<(), Refused> {
        Ok(())
    }

    /// Accept or refuse a checked topology. A backend that accepts has allocated everything the
    /// topology needs, and no worker has started yet.
    fn launch(plan: Plan) -> Result<Self::Run, Refused>;

    /// Send `bytes` of the message type `ty` (named `name` in messages) from this worker to `to`.
    /// Panics when this worker has no such route.
    fn worker_send(to: WorkerId, ty: TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full>;

    /// Take the next frame of type `ty` addressed to this worker into `out`, which has the size of
    /// that type. Answers `false`, and leaves `out` alone, when there is no frame. Allocates nothing.
    fn worker_recv(ty: TypeId, out: &mut [u8]) -> bool;

    /// Whether a frame of type `ty` is addressed to this worker. Answers without yielding the baton.
    fn worker_offer(ty: TypeId) -> bool;

    /// The index of the route from this worker to `to` with type `ty`, among this worker's outgoing
    /// routes in plan order, or None when there is none. Answers without yielding the baton. A
    /// `Status` names rooms by this index (status.rs).
    fn worker_route(to: WorkerId, ty: TypeId) -> Option<usize>;

    /// Whether the outgoing route at `route` (see `worker_route`) has room for a send: fewer frames
    /// than its capacity, or a lossy route. Panics when this worker has no such route. Answers without
    /// yielding the baton.
    fn worker_room(route: usize) -> bool;

    /// The run's change counter: it moves on every act of an agent (a worker's effect, or a leader's
    /// send, take or publication). A worker reads it before it checks its wait conditions, and passes
    /// that value to `worker_idle`, so an act between the check and the idle is not missed.
    fn worker_epoch() -> u64;

    /// Give up the baton and block until the counter differs from `epoch` (see `worker_epoch`), or
    /// until this worker's clock reaches `until` (nanoseconds since the run started). Returns with the
    /// baton held again. Never returns early without one of the two.
    fn worker_idle(until: Option<u64>, epoch: u64);

    /// This worker's clock: its identity, and the nanoseconds since the run started.
    fn worker_now() -> (ClockId, u64);

    /// Wait until every worker is at a barrier, then return the publication of the host. The
    /// guard keeps the bytes alive; taking it allocates nothing.
    fn worker_barrier() -> Arc<[u8]>;

    /// Send from the leader to `to`. Panics when the topology has no such route.
    fn leader_send(run: &Self::Run, to: WorkerId, ty: TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full>;

    /// Take the next frame of type `ty` addressed to the leader into `out`, as `worker_recv` does.
    fn leader_recv(run: &Self::Run, ty: TypeId, out: &mut [u8]) -> bool;

    /// Replace the publication. Workers see it at the next barrier that fires.
    fn leader_publish(run: &Self::Run, bytes: &[u8]);

    /// Block until the leader's counter differs from `seen`, and answer the new value. The surface
    /// keeps `seen` (run.rs) and passes the answer back next time, so an act between two waits is not
    /// missed. Like `worker_idle`, it moves on every act that can change the leader's wait: a frame to
    /// the leader, a worker's result, an abort.
    fn leader_wait(run: &Self::Run, seen: u64) -> u64;

    /// Wait until every agent has a result, and return one result per worker in declaration
    /// order.
    fn leader_join(run: Self::Run) -> Vec<Outcome>;
}

/// The backend this build selected.
pub(crate) type Selected = crate::selected::Selected;

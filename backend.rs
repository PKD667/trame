//! The seam between the surface and the selected backend, and the contract a backend keeps. The
//! surface calls these functions and nothing else in a backend; a backend implements `Backend` on its
//! `Selected` type, so the compiler checks the signatures. The clauses below are what the types cannot
//! say. Each is numbered so a test, a mutant or a model can name the clause it checks.
//!
//! Scheduling (the model is sched.rs; a backend schedules by itself or with worker.rs's `agent`):
//!   S1 Exclusion. A step runs only while it holds its fields: while step i runs, no other step of its
//!      worker that writes a field of R(i) ∪ W(i), or reads a field of W(i), runs. `State::run` is
//!      called only under S1; it is the safety condition of that unsafe call.
//!   S2 Enabledness. A step starts only when `Sched::may_start` holds for it, with the wait
//!      conditions the backend observed at that start.
//!   S3 Wake. A waiting step is woken by an End that writes one of its fields, or by a condition it
//!      named; a woken step whose fields are free starts unless the worker stops, fails or another
//!      step takes the fields first. No wake is lost: an agent that finds nothing to start sleeps
//!      only until the next change of its worker (an End, a frame, room, its clock, a leader act).
//!   S4 Edit. `leader_edit` runs its function while no step of that worker runs and none starts,
//!      then wakes every step of the worker. Queued frames are untouched.
//!   S5 Stop. After `leader_stop`, no ordinary step starts; the stop step starts only when no step
//!      runs; the worker's result is `Stopped` when the stop step is done (or there is none).
//!   S6 Shares. Under contention, the service of positive-weight steps follows their weights, and a
//!      weight-0 step starts only when no positive-weight step may. seq follows `Sched::choose`
//!      exactly; other backends are held to measured shares within a stated tolerance.
//!   S7 Trace. When the launch asks for a trace, the backend records each worker's Start, End, Stop
//!      and Edit events in an order consistent with the run; `sched::check` must accept it.
//!
//! Transport (spec.md 5.2, unchanged):
//!   T1 FIFO per route. T2 `Full` only on a non-lossy route at capacity; a lossy route replaces its
//!      oldest frame. T3 A frame is delivered at most once, to the route's receiver.
//!
//! Pool:
//!   P1 Only the leader writes the pool, in `leader_edit(Target::Pool)`: steps read it.
//!   P2 A pool edit is S4 over the POOL bit (sched.rs) in every worker: it runs while no step that
//!      reads the pool runs, no such step starts until it returns, and those steps are woken after.
//!
//! Worker operations act for the calling thread, which must be an agent of a worker. Leader
//! operations act for the `Run` they are given. Bytes cross this seam; typed messages are encoded in
//! `io`.

use crate::io::Full;
use crate::launch::{Plan, Refused};
use crate::run::Outcome;
use crate::sched::{Event, Sched};
use crate::time::ClockId;
use crate::topology::WorkerId;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::{Any, TypeId};

/// The launch's pool, a `PoolCore<P>` (worker.rs), as the backend holds it.
pub(crate) type PoolRef = Arc<dyn Any + Send + Sync>;

/// What a leader edit writes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Target {
    /// Worker w's state: every field (sched.rs `STATE`).
    Worker(usize),
    /// The pool: the POOL bit, in every worker.
    Pool,
}

pub(crate) trait Backend {
    /// The state of one launched run, owned by the leader.
    type Run;

    /// Whether the workers' states live in the leader's process, so that `Worker::take` and
    /// `Worker::error` read the real ones. False on a backend that runs workers in other processes
    /// and does not ship states back (mpi, for now): the leader's copy is then stale, and the surface
    /// refuses to answer from it.
    const STATES_AT_LEADER: bool = true;

    /// Check that every process of the launch built the same plan, before the local checks run. A
    /// backend with one process has nothing to agree on. A backend with several processes makes this
    /// a collective: every process calls it, in the same order, and every process gets the same answer.
    fn agree(_plan: &Plan) -> Result<(), Refused> {
        Ok(())
    }

    /// Accept or refuse a checked plan. A backend that accepts has allocated everything the plan
    /// needs, and no step has started yet.
    fn launch(plan: Plan) -> Result<Self::Run, Refused>;

    /// This worker.
    fn worker_me() -> WorkerId;

    /// Send `bytes` of the message type `ty` (named `name` in messages) from this worker to `to`.
    /// Panics when this worker has no such route.
    fn worker_send(to: WorkerId, ty: TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full>;

    /// Take the next frame of type `ty` addressed to this worker into `out`, which has the size of
    /// that type. Answers `false`, and leaves `out` alone, when there is no frame. Allocates nothing.
    fn worker_recv(ty: TypeId, out: &mut [u8]) -> bool;

    /// Whether a frame of type `ty` is addressed to this worker.
    fn worker_offer(ty: TypeId) -> bool;

    /// The index of the route from this worker to `to` with type `ty`, among this worker's outgoing
    /// routes in plan order, or None. A `Status` names rooms by this index (status.rs).
    fn worker_route(to: WorkerId, ty: TypeId) -> Option<usize>;

    /// Whether the outgoing route at `route` (see `worker_route`) has room for a send: fewer frames
    /// than its capacity, or a lossy route. Panics when this worker has no such route.
    fn worker_room(route: usize) -> bool;

    /// The run's change counter, as seen by this worker: it moves on every act that can change a wait
    /// condition of this worker (a frame, room, a leader act). An agent reads it before it checks the
    /// conditions and passes it to `worker_idle`, so a change between the two is not missed (S3).
    fn worker_epoch() -> u64;

    /// Block until the counter differs from `epoch`, or this worker's clock reaches `until`
    /// (nanoseconds since the run started).
    fn worker_idle(until: Option<u64>, epoch: u64);

    /// This worker's clock: its identity, and the nanoseconds since the run started.
    fn worker_now() -> (ClockId, u64);

    /// Whether the leader stopped the run, for a backend whose leader cannot reach the worker's
    /// scheduler directly (mpi: another process). The reference agent polls it at each round.
    fn worker_stop_requested() -> bool {
        false
    }

    /// Leave the worker without a result of this agent's own: another agent's step unwound mid-way
    /// (the run aborts), so this agent must not run on. Unwinds with the backend's marker.
    fn worker_abandon() -> !;

    /// Send from the leader to `to`. Panics when the topology has no such route.
    fn leader_send(run: &Self::Run, to: WorkerId, ty: TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full>;

    /// Take the next frame of type `ty` addressed to the leader into `out`, as `worker_recv` does.
    fn leader_recv(run: &Self::Run, ty: TypeId, out: &mut [u8]) -> bool;

    /// Edit worker `w` (S4): run `f` while no step of `w` runs. Answers false, without running `f`,
    /// when `w` already has a result.
    /// A pool edit (P2) answers true.
    fn leader_edit(run: &Self::Run, target: Target, f: &mut dyn FnMut()) -> bool;

    /// Stop every worker (S5). Returns at once; `leader_join` waits for the results.
    fn leader_stop(run: &Self::Run);

    /// Block until the leader's counter differs from `seen`, and answer the new value. It moves on
    /// every act that can change the leader's wait: a frame to the leader, a worker's result, an abort.
    fn leader_wait(run: &Self::Run, seen: u64) -> u64;

    /// Wait until every worker has a result, and return them in declaration order. After it returns,
    /// no step of the run runs again, so the leader may take the states.
    fn leader_join(run: Self::Run) -> Vec<Outcome>;

    /// `leader_join`, then every worker's trace with its initial model, if the launch asked for traces
    /// (S7); complete, since nothing runs.
    fn leader_join_traced(run: Self::Run, workers: usize) -> (Vec<Outcome>, Vec<Option<(Sched, Vec<Event>)>>);
}

/// The backend this build selected.
pub(crate) type Selected = crate::selected::Selected;

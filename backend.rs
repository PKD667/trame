//! The seam between the surface and the selected backend. The surface calls these functions and
//! nothing else in a backend; a backend implements `Backend` on its `Selected` type, so the compiler
//! checks the signatures. What the signatures cannot say is `CONTRACT.md`, whose clauses (S1, M1,
//! ...) the comments below name. `PROOFS.md` says how each is checked.
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
use core::any::TypeId;

/// The launch's pool, a `PoolCore<P>` (worker.rs), as the backend holds it.
pub(crate) type PoolRef = Arc<dyn crate::worker::ErasedPool>;

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

    /// S4 for a leader in another process (the handshake): the fields it asks this worker to hold,
    /// 0 when it asks none. The agent loop holds them (`Event::Hold`), answers
    /// `worker_edit_held(true)` once no step touching them runs, and, once the request is 0 again,
    /// gives them back (`Event::Edit`) and answers `worker_edit_held(false)`. A backend whose leader
    /// edits in the workers' process (seq, threads) never asks.
    fn worker_edit_request() -> u64 {
        0
    }

    /// Tell the leader this worker holds (true) or gave back (false) the fields it asked for.
    fn worker_edit_held(_held: bool) {
        unreachable!("trame: no remote edit was asked")
    }

    /// M1: `layout.size()` bytes of the domain of this process's launch, for a builder or an edit
    /// (alloc.rs, arena mode). `None`: this process's heap is the domain (seq, threads; or no launch
    /// yet). A full domain answers a null pointer, which the allocation reports (M6).
    fn domain_alloc(_layout: core::alloc::Layout) -> Option<*mut u8> {
        None
    }

    /// Whether `p` lies in a domain of this process, so `domain_dealloc` frees it.
    fn domain_contains(_p: *mut u8) -> bool {
        false
    }

    /// Free a block `domain_alloc` answered.
    ///
    /// # Safety
    /// `p` came from `domain_alloc` with `layout`, and is freed once.
    unsafe fn domain_dealloc(_p: *mut u8, _layout: core::alloc::Layout) {
        unreachable!("trame: a heap domain owns no block")
    }

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
    /// (S9); complete, since nothing runs.
    fn leader_join_traced(run: Self::Run, workers: usize) -> (Vec<Outcome>, Vec<Option<(Sched, Vec<Event>)>>);
}

/// The backend this build selected.
pub(crate) type Selected = crate::selected::Selected;

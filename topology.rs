//! Worker identities, the handles of declared workers, and the topology the host builds: spec.md
//! section 4.

use crate::backend::{Backend, Selected};
use crate::io::Message;
use crate::launch::{Plan, Refused, Route, check};
use crate::run::Run;
use crate::worker::{Core, State};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use core::any::{TypeId, type_name};
use core::mem::size_of;

/// Opaque identity of a worker, or of the leader. Made only by `Topology::worker`, and `LEADER`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorkerId(pub(crate) u32);

/// The leader of the deployment. It is an agent of its own, and it may be the end of a route.
pub const LEADER: WorkerId = WorkerId(u32::MAX);

/// A declared worker of state type `S`: its identity, and the handle the leader edits and takes the
/// state through. Made by `Topology::worker`.
pub struct Worker<S: State> {
    id: WorkerId,
    core: Arc<Core<S>>,
    joined: Arc<AtomicBool>,
}

impl<S: State> Worker<S> {
    /// The worker's identity, for routes and sends.
    pub fn id(&self) -> WorkerId {
        self.id
    }

    pub(crate) fn core(&self) -> &Core<S> {
        &self.core
    }

    /// The state, after `Run::join`. Panics before the join, or when the run aborted (the leader
    /// panicked then, so this is reached only by a leader that caught that panic).
    pub fn take(self) -> S {
        assert!(self.joined.load(Ordering::Acquire), "trame: Worker::take before Run::join; a running worker's state is not the leader's");
        // SAFETY: joined, so no step runs again (backend.rs, leader_join).
        unsafe { self.core.take() }
    }

    /// The error of the step that failed, after `Run::join`, when the worker's outcome is `Failed`.
    pub fn error(&self) -> Option<S::Error> {
        assert!(self.joined.load(Ordering::Acquire), "trame: Worker::error before Run::join");
        self.core.take_error()
    }
}

/// A topology under construction: the workers with their states, the routes between them, and the
/// host's pool. `launch` checks it and asks the backend to accept it.
#[derive(Default)]
pub struct Topology {
    plan: Plan,
    joined: Arc<AtomicBool>,
}

impl Topology {
    pub fn new() -> Self {
        Topology::default()
    }

    /// Declare a worker whose state is `state`: its steps are those of `#[trame::state]` on `S`.
    /// Workers are numbered in declaration order.
    pub fn worker<S: State>(&mut self, state: S) -> Worker<S> {
        let index = u32::try_from(self.plan.workers.len()).expect("more workers than WorkerId can name");
        let core = Arc::new(Core::new(state));
        self.plan.workers.push(core.clone());
        Worker { id: WorkerId(index), core, joined: self.joined.clone() }
    }

    /// The identities the next `n` calls of `worker` will answer, in order: a state can then name
    /// workers declared after it.
    pub fn ids(&self, n: usize) -> Vec<WorkerId> {
        let first = self.plan.workers.len();
        (first..first + n).map(|k| WorkerId(u32::try_from(k).expect("more workers than WorkerId can name"))).collect()
    }

    /// A route from `from` to `to` carrying messages of type `M`, with room for `cap` frames.
    pub fn route<M: Message>(&mut self, from: WorkerId, to: WorkerId, cap: usize) {
        self.add::<M>(from, to, cap, false);
    }

    /// As `route`, but a send at capacity replaces the oldest frame instead of answering `Full`.
    pub fn lossy<M: Message>(&mut self, from: WorkerId, to: WorkerId, cap: usize) {
        self.add::<M>(from, to, cap, true);
    }

    fn add<M: Message>(&mut self, from: WorkerId, to: WorkerId, cap: usize, lossy: bool) {
        self.plan.routes.push(Route {
            from,
            to,
            ty: TypeId::of::<M>(),
            name: type_name::<M>(),
            size: size_of::<M>(),
            cap,
            lossy,
        });
    }

    /// The host's pool: read-only data every step that names a `pool` parameter reads. The leader
    /// replaces it with `Run::publish`. A worker whose steps read a pool of another type is refused.
    pub fn pool<P: Send + Sync + 'static>(&mut self, pool: P) {
        self.plan.pool = Some(Arc::new(pool));
        self.plan.pool_type = core::any::TypeId::of::<P>();
        self.plan.pool_name = core::any::type_name::<P>();
    }

    /// Test hook: run each worker with at most `n` agents; by default as many as the host has cores.
    /// Not surface: deployment, not meaning. seq ignores it.
    #[doc(hidden)]
    pub fn __agents(&mut self, n: usize) {
        assert!(n >= 1, "trame: a worker needs at least one agent");
        self.plan.agents = Some(n);
    }

    /// Record every worker's trace of starts, ends, stops and edits, for `Run::__check_traces`.
    #[doc(hidden)]
    pub fn __trace(&mut self) {
        self.plan.trace = true;
    }

    /// Agree with the other processes of the launch, check the topology, then ask the backend to accept
    /// it. No worker starts unless every step accepts.
    pub fn launch(self) -> Result<Run, Refused> {
        Selected::agree(&self.plan)?;
        check(&self.plan)?;
        let (pool_type, workers) = (self.plan.pool_type, self.plan.workers.len());
        Selected::launch(self.plan).map(|r| Run::new(r, pool_type, workers, self.joined))
    }
}

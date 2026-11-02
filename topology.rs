//! Worker identities, the handles of declared workers, and the topology the host builds: spec.md
//! section 4.

use crate::backend::{Backend, Selected};
use crate::io::Message;
use crate::launch::{Plan, Refused, Route, check};
use crate::run::Run;
use crate::backend::Target;
use crate::worker::{Core, PoolCore, State};
use alloc::sync::Arc;
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

    /// Declare a worker of state type `S`. Its state is given later with `set`, so a state can name
    /// workers declared after it. Workers have no order a program may rely on.
    pub fn worker<S: State>(&mut self) -> Worker<S> {
        let index = u32::try_from(self.plan.workers.len()).expect("more workers than WorkerId can name");
        let core = Arc::new(Core::empty());
        self.plan.workers.push(core.clone());
        self.plan.builders.push(None);
        Worker { id: WorkerId(index), core, joined: self.joined.clone() }
    }

    /// Declare the pool: read-only data shared by every worker, of type `P`. A step reads it by
    /// taking `pool: &P`; only the leader writes it, with `Run::edit`. Its value is given with `set`.
    /// One pool per topology. A worker whose steps read a pool of another type is refused.
    pub fn pool<P: Send + Sync + 'static>(&mut self) -> Pool<P> {
        assert!(self.plan.pool.is_none(), "trame: a topology has one pool");
        let core = Arc::new(PoolCore::empty());
        self.plan.pool = Some(core.clone());
        self.plan.pool_type = TypeId::of::<P>();
        self.plan.pool_name = type_name::<P>();
        Pool { core }
    }

    /// Give `h` (a worker or the pool) its value: `build` makes it. The backend calls `build` at
    /// launch, in the memory where it is used (backend.rs, M1), so every allocation the value owns
    /// lands there; a value built outside `build` and moved in does not. A second `set` replaces the
    /// first.
    pub fn set<H: Handle>(&mut self, h: &H, build: impl FnOnce() -> H::Value + Send + 'static) {
        h.set(&mut self.plan, alloc::boxed::Box::new(build));
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
        let (pool, workers) = (self.plan.pool.as_ref().map(|p| Arc::as_ptr(p).cast::<()>()), self.plan.workers.len());
        Selected::launch(self.plan).map(|r| Run::new(r, pool, workers, self.joined))
    }
}

/// The pool, as `Topology::pool` declared it: a handle for `Topology::set` and `Run::edit`.
pub struct Pool<P> {
    core: Arc<PoolCore<P>>,
}

/// What `Topology::set` and `Run::edit` act on: a worker's state (`Worker<S>`) or the pool
/// (`Pool<P>`). Implemented by those two only.
pub trait Handle: sealed::Sealed {}
impl<S: State> Handle for Worker<S> {}
impl<P: Send + Sync + 'static> Handle for Pool<P> {}

// The methods take crate types on purpose: nothing outside the crate can name or implement Sealed.
#[allow(private_interfaces)]
pub(crate) mod sealed {
    use super::*;

    pub trait Sealed {
        /// A worker's state, or the pool.
        type Value: Send + 'static;
        /// Record `build` in `plan` (`Topology::set`).
        fn set(&self, plan: &mut Plan, build: alloc::boxed::Box<dyn FnOnce() -> Self::Value + Send>);
        /// What a leader edit of it holds. Panics when it is not of `run`'s topology.
        fn target(&self, run: &Run) -> Target;
        /// # Safety
        /// The backend holds `target`'s fields (backend.rs, S4, P2).
        unsafe fn edit_with(&self, f: &mut dyn FnMut(&mut Self::Value));
    }

    impl<S: State> Sealed for Worker<S> {
        type Value = S;
        fn set(&self, plan: &mut Plan, build: alloc::boxed::Box<dyn FnOnce() -> S + Send>) {
            let k = self.id.0 as usize;
            let ours = plan.workers.get(k).is_some_and(|c| core::ptr::addr_eq(Arc::as_ptr(c), Arc::as_ptr(&self.core)));
            assert!(ours, "trame: Topology::set of a worker declared by another topology");
            let core = self.core.clone();
            plan.builders[k] = Some(alloc::boxed::Box::new(move || core.fill(build())));
        }
        fn target(&self, run: &Run) -> Target {
            let index = self.id.0 as usize;
            assert!(index < run.workers(), "trame: Run::edit of a worker of another topology");
            Target::Worker(index)
        }
        unsafe fn edit_with(&self, f: &mut dyn FnMut(&mut S)) {
            // SAFETY: the caller's.
            unsafe { self.core.edit_with(f) }
        }
    }

    impl<P: Send + Sync + 'static> Sealed for Pool<P> {
        type Value = P;
        fn set(&self, plan: &mut Plan, build: alloc::boxed::Box<dyn FnOnce() -> P + Send>) {
            let ours = plan.pool.as_ref().is_some_and(|p| core::ptr::addr_eq(Arc::as_ptr(p), Arc::as_ptr(&self.core)));
            assert!(ours, "trame: Topology::set of a pool declared by another topology");
            let core = self.core.clone();
            plan.pool_builder = Some(alloc::boxed::Box::new(move || core.fill(build())));
        }
        fn target(&self, run: &Run) -> Target {
            let ours = run.pool().is_some_and(|p| core::ptr::addr_eq(p, Arc::as_ptr(&self.core)));
            assert!(ours, "trame: Run::edit of a pool of another topology");
            Target::Pool
        }
        unsafe fn edit_with(&self, f: &mut dyn FnMut(&mut P)) {
            // SAFETY: the caller's.
            unsafe { self.core.edit_with(f) }
        }
    }
}

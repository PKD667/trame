//! Worker identities, the context a worker starts with, and the topology the host builds: spec.md
//! section 4.

use crate::backend::{Backend, Selected};
use crate::io::Message;
use crate::launch::{check, Plan, Refused, Route};
use crate::run::Run;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::{type_name, TypeId};
use core::mem::size_of;

/// Opaque identity of a worker, or of the leader. Made only by `Topology::worker`, and `LEADER`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorkerId(pub(crate) u32);

/// The leader of the deployment. It is an agent of its own, and it may be the end of a route.
pub const LEADER: WorkerId = WorkerId(u32::MAX);

/// What a worker knows when it starts: its own identity and the identities of all workers, in
/// declaration order.
#[derive(Debug)]
pub struct Context {
    me: WorkerId,
    workers: Vec<WorkerId>,
}

impl Context {
    pub(crate) fn new(me: WorkerId, workers: Vec<WorkerId>) -> Self {
        Context { me, workers }
    }

    /// This worker.
    pub fn me(&self) -> WorkerId {
        self.me
    }

    /// Every worker of the topology, in declaration order.
    pub fn workers(&self) -> &[WorkerId] {
        &self.workers
    }
}

/// A topology under construction: the workers, the routes between them, and the host's first
/// publication. `launch` checks it and asks the backend to accept it.
#[derive(Default)]
pub struct Topology {
    plan: Plan,
}

impl Topology {
    pub fn new() -> Self {
        Topology::default()
    }

    /// Declare a worker that runs `f`. Workers are numbered in declaration order. The checker reads
    /// `f` as a worker's `run` (spec.md, Requirement (Worker)): the marker is instantiated with `F`, the
    /// concrete type, so a fn pointer would hide what runs.
    pub fn worker<F: Fn(&Context) + Send + 'static>(&mut self, f: F) -> WorkerId {
        crate::__check::worker(&f);
        let index = u32::try_from(self.plan.workers.len()).expect("more workers than WorkerId can name");
        self.plan.workers.push(Box::new(f));
        WorkerId(index)
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

    /// The host's initial publication: what workers read at their first barrier, until a leader
    /// publication replaces it.
    pub fn publish(&mut self, bytes: &[u8]) {
        self.plan.publication = bytes.to_vec();
    }

    /// Agree with the other processes of the launch, check the topology, then ask the backend to accept
    /// it. No worker starts unless every step accepts.
    pub fn launch(self) -> Result<Run, Refused> {
        Selected::agree(&self.plan)?;
        check(&self.plan)?;
        Selected::launch(self.plan).map(Run::new)
    }
}

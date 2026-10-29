//! Launch checks: spec.md section 4, "Launch checks". A topology that fails them never starts a
//! worker. The checks are the trame part of acceptance; the backend adds its own refusals.

use crate::topology::{Context, LEADER, WorkerId};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::TypeId;

/// Why a topology was refused. Nothing ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// A route names a worker that the topology does not contain.
    UnknownWorker(WorkerId),
    /// Two routes have the same sender, receiver and message type.
    Duplicate { from: WorkerId, to: WorkerId, name: &'static str },
    /// A route has capacity 0, but the spec requires `cap >= 1`.
    ZeroCapacity { name: &'static str },
    /// The backend could not allocate what the topology needs.
    Backend { reason: &'static str },
}

/// One route as the topology records it. `ty` identifies the tag: the surface uses the message
/// type as the tag, so two tags cannot share one type.
#[derive(Clone, Debug)]
pub(crate) struct Route {
    pub from: WorkerId,
    pub to: WorkerId,
    pub ty: TypeId,
    pub name: &'static str,
    pub size: usize,
    pub cap: usize,
    pub lossy: bool,
}

/// One worker's run.
pub(crate) type WorkerFn = Box<dyn Fn(&Context) + Send>;

/// A topology as the surface built it, before the checks. The backend receives it only after
/// `check` passes.
#[derive(Default)]
pub(crate) struct Plan {
    /// Each worker's run, boxed: a generic `Topology::worker` cannot keep a fn pointer. `Send`, since a
    /// backend may start each worker on a thread of its own.
    pub workers: Vec<WorkerFn>,
    pub routes: Vec<Route>,
    pub publication: Vec<u8>,
}

/// The launch checks of spec.md section 4, in the order the spec states them.
pub(crate) fn check(plan: &Plan) -> Result<(), Refused> {
    let n = plan.workers.len();
    for (i, r) in plan.routes.iter().enumerate() {
        for end in [r.from, r.to] {
            if end != LEADER && end.0 as usize >= n {
                return Err(Refused::UnknownWorker(end));
            }
        }
        if r.cap == 0 {
            return Err(Refused::ZeroCapacity { name: r.name });
        }
        let dup = plan.routes[..i].iter().any(|q| q.from == r.from && q.to == r.to && q.ty == r.ty);
        if dup {
            return Err(Refused::Duplicate { from: r.from, to: r.to, name: r.name });
        }
    }
    Ok(())
}

//! Launch checks: spec.md section 4, "Launch checks". A topology that fails them never starts a
//! worker. The checks are the trame part of acceptance; the backend adds its own refusals.

use crate::backend::PoolRef;
use crate::topology::{LEADER, WorkerId};
use crate::worker::Erased;
use alloc::sync::Arc;
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
    /// Two steps of one worker receive the same message type: a tag has one owner per worker.
    TwoOwners { worker: WorkerId, name: &'static str },
    /// A worker's steps read a pool of type `want`, and the host's pool is `have` (`()`: none).
    PoolType { worker: WorkerId, want: &'static str, have: &'static str },
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

/// A topology as the surface built it, before the checks. The backend receives it only after
/// `check` passes.
pub(crate) struct Plan {
    /// Each worker: its state and steps, without their type (worker.rs).
    pub workers: Vec<Arc<dyn Erased>>,
    pub routes: Vec<Route>,
    /// The host's pool, its type and the type's name. `None` with `()`: no pool.
    pub pool: Option<PoolRef>,
    pub pool_type: TypeId,
    pub pool_name: &'static str,
    /// Record each worker's trace (backend.rs, S7). For conformance; costs a lock and a push per event.
    pub trace: bool,
}

impl Default for Plan {
    fn default() -> Self {
        Plan {
            workers: Vec::new(),
            routes: Vec::new(),
            pool: None,
            pool_type: TypeId::of::<()>(),
            pool_name: "()",
            trace: false,
        }
    }
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
    for (k, w) in plan.workers.iter().enumerate() {
        let worker = WorkerId(k as u32);
        if w.pool_type() != TypeId::of::<()>() && w.pool_type() != plan.pool_type {
            return Err(Refused::PoolType { worker, want: w.pool_name(), have: plan.pool_name });
        }
        let n = w.steps().len();
        for i in 0..n {
            for (t, name) in w.tags(i).iter().flatten() {
                if (i + 1..n).any(|j| w.tags(j).iter().flatten().any(|(u, _)| u == t)) {
                    return Err(Refused::TwoOwners { worker, name });
                }
            }
        }
    }
    Ok(())
}

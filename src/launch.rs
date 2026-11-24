//! Launch checks: INTERFACE.md section 4, "Launch checks". A topology that fails them never starts a
//! worker. The checks are the trame part of acceptance; the backend adds its own refusals.

use crate::backend::PoolRef;
use crate::topology::{End, WorkerId};
use crate::worker::Erased;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::TypeId;

/// Why a topology was refused. Nothing ran. `Display` says it in one sentence, with what to fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// A route names a worker that the topology does not contain.
    UnknownWorker(WorkerId),
    /// Two routes have the same sender, receiver and message type.
    Duplicate { from: End, to: End, name: &'static str },
    /// A route has capacity 0, but the spec requires `cap >= 1`.
    ZeroCapacity { name: &'static str },
    /// Two steps of one worker receive the same message type: a tag has one owner per worker.
    TwoOwners { worker: WorkerId, name: &'static str },
    /// A worker's steps read a pool of type `want`, and the host's pool is `have` (`()`: none).
    PoolType { worker: WorkerId, want: &'static str, have: &'static str },
    /// `trame::Alloc` is not the program's global allocator (domain.rs).
    Alloc,
    /// A worker's builder took more than the worker's share of the host's memory (INTERFACE.md 6, Definition
    /// (Memory of a host)). The builder and the state it was building are dropped.
    Share { worker: WorkerId, want: u64, share: u64 },
    /// The pool was declared but never set (`Topology::set`).
    PoolUnset,
    /// A worker was declared but its state was never set (`Topology::set`).
    Unset { worker: WorkerId },
    /// The CPU lacks an instruction set that the build's lane feature (`lanes-avx2`, `lanes-avx512`) needs.
    CpuFeature { feature: &'static str, remedy: &'static str },
    /// The backend cannot run the topology here: its message says why, with the numbers.
    Backend { reason: alloc::string::String },
}

impl core::fmt::Display for Refused {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Refused::UnknownWorker(w) => write!(f, "a route names {w:?}, which this topology did not declare"),
            Refused::Duplicate { from, to, name } => {
                write!(f, "two routes of {name} from {from:?} to {to:?}; declare it once")
            }
            Refused::ZeroCapacity { name } => write!(f, "a route of {name} has capacity 0; give it 1 or more"),
            Refused::TwoOwners { worker, name } => {
                write!(f, "two steps of {worker:?} receive {name}; one step owns a message type")
            }
            Refused::PoolType { worker, want, have } => {
                write!(f, "{worker:?}'s steps read a pool of {want}, and the pool is {have}")
            }
            Refused::Share { worker, want, share } => write!(
                f,
                "{worker:?}'s builder takes {want} bytes of state, above its share of {share} bytes: this host's \
                 memory, less a sixteenth for the launch and the pool, divided by the host's workers \
                 (trame::system::info().worker_bytes; the launcher may declare the host's memory with TRAME_AVAILABLE)"
            ),
            Refused::Alloc => write!(
                f,
                "trame::Alloc is not the global allocator; add `#[global_allocator] static A: trame::Alloc = trame::Alloc::new(std::alloc::System);`"
            ),
            Refused::PoolUnset => write!(f, "the pool is declared and never set; call Topology::set on it"),
            Refused::CpuFeature { feature, remedy } => {
                write!(f, "this CPU lacks {feature}, which the build's lane feature needs; {remedy}")
            }
            Refused::Unset { worker } => {
                write!(f, "{worker:?} is declared and its state never set; call Topology::set on it")
            }
            Refused::Backend { reason } => write!(f, "{reason}"),
        }
    }
}

/// One route as the topology records it. `ty` identifies the tag: the surface uses the message
/// type as the tag, so two tags cannot share one type.
#[derive(Clone, Debug)]
pub(crate) struct Route {
    pub from: End,
    pub to: End,
    pub ty: TypeId,
    pub name: &'static str,
    /// The type's tag across hosts (CONTRACT.md, F4).
    #[allow(dead_code)] // read by a backend with routes between hosts
    pub wire: u64,
    pub size: usize,
    pub cap: usize,
    pub lossy: bool,
}

/// A topology as the surface built it, before the checks. The backend receives it only after
/// `check` passes.
pub(crate) struct Plan {
    /// Each worker: its state and steps, without their type (worker.rs).
    pub workers: Vec<Arc<dyn Erased>>,
    /// Each worker's builder (`Topology::set`): builds the state and puts it in place. The backend
    /// runs it at launch, in the worker's domain (CONTRACT.md, M1). `None`: never set, refused.
    pub builders: Vec<Option<Builder>>,
    pub routes: Vec<Route>,
    /// The pool (a `PoolCore<P>`), its type and the type's name. `None` with `()`: no pool.
    pub pool: Option<PoolRef>,
    /// The pool's builder (`Topology::set`); run at launch like a worker's.
    pub pool_builder: Option<Builder>,
    pub pool_type: TypeId,
    pub pool_name: &'static str,
    /// Record each worker's trace (CONTRACT.md, S9). For conformance; costs a lock and a push per event.
    pub trace: bool,
    /// Agents per worker, at most; `None`: the host's parallelism. Never more than a worker's steps.
    pub agents: Option<usize>,
}

impl Default for Plan {
    fn default() -> Self {
        Plan {
            workers: Vec::new(),
            builders: Vec::new(),
            routes: Vec::new(),
            pool: None,
            pool_builder: None,
            pool_type: TypeId::of::<()>(),
            pool_name: "()",
            trace: false,
            agents: None,
        }
    }
}

/// Builds one worker's state and puts it in place (worker.rs `Core::fill`).
pub(crate) type Builder = alloc::boxed::Box<dyn FnOnce() + Send>;

/// The CPU features the build's lane feature needs (src/parallel/). Checked before the others; a build without a
/// lane feature has none to check.
#[inline]
fn check_cpu_features() -> Result<(), Refused> {
    #[cfg(feature = "lanes-avx2")]
    {
        if !crate::__rt::avx2::has_avx2() {
            return Err(Refused::CpuFeature {
                feature: "AVX2",
                remedy: "build without --features lanes-avx2 or run on a CPU with AVX2",
            });
        }
    }
    #[cfg(feature = "lanes-avx512")]
    {
        if !crate::__rt::avx512::has_avx512() {
            return Err(Refused::CpuFeature {
                feature: "AVX-512",
                remedy: "build without --features lanes-avx512 or run on a CPU with AVX-512 (F, BW, DQ, VL)",
            });
        }
    }
    Ok(())
}

/// The launch checks of INTERFACE.md section 4, in the order the spec states them.
pub(crate) fn check(plan: &Plan) -> Result<(), Refused> {
    check_cpu_features()?;
    let n = plan.workers.len();
    for (i, r) in plan.routes.iter().enumerate() {
        // A worker of this host must be declared; a worker of another host must be on a host of the
        // deployment, and its host checks the rest (F3). A route names at least one end of this host.
        for end in [r.from, r.to] {
            if let End::Worker(w) = end
                && (if w.1 == crate::topology::HERE { w.0 as usize >= n } else { w.1 >= crate::io::hosts() })
            {
                return Err(Refused::UnknownWorker(w));
            }
        }
        let remote = |e: End| matches!(e, End::Worker(w) if w.1 != crate::topology::HERE);
        if let (true, true, End::Worker(w)) = (remote(r.from), remote(r.to), r.from) {
            return Err(Refused::UnknownWorker(w));
        }
        if r.cap == 0 {
            return Err(Refused::ZeroCapacity { name: r.name });
        }
        let dup = plan.routes[..i].iter().any(|q| q.from == r.from && q.to == r.to && q.ty == r.ty);
        if dup {
            return Err(Refused::Duplicate { from: r.from, to: r.to, name: r.name });
        }
    }
    if !crate::domain::installed() {
        return Err(Refused::Alloc);
    }
    if plan.pool.is_some() && plan.pool_builder.is_none() {
        return Err(Refused::PoolUnset);
    }
    for (k, w) in plan.workers.iter().enumerate() {
        let worker = WorkerId::here(k as u32);
        if plan.builders[k].is_none() {
            return Err(Refused::Unset { worker });
        }
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

//! A private representability check over a launch plan (PLAN.md, step 2). It answers one question
//! before any worker starts: can the NVIDIA backend hold this plan's state on the device, given the
//! limits the host probed? It refuses with `Refusal::Backend`, the shape of `launch::Refused::Backend`.
//!
//! What the plan can see, and what it cannot. The surface knows each worker only as a closure type
//! `F`, each route as a message type with a size and a capacity, and the publication as bytes. It
//! cannot see the state a worker's body creates (a `Vec` made inside `run`, the slots of a
//! `parallel!`, the cells of a `concurrent!`): those are types inside the body. So this check is a
//! statement about the plan's fields only. Its gaps are listed in `representable`'s doc.

/// What the surface records about one worker at registration (`Topology::worker::<F>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerView {
    /// `!core::mem::needs_drop::<F>()`: the closure's captures have no destructor, so none owns heap
    /// memory. A necessary condition only: a captured `&T` or raw pointer passes it, and is not seen.
    pub captures_drop_free: bool,
}

/// What the surface records about one route (`Topology::route`, `lossy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteView {
    /// `cap`, the capacity in frames.
    pub cap: usize,
    /// `size_of::<M>()`. Messages are plain data (`Message`), so these bytes are representable as
    /// they stand; only their total matters here.
    pub size: usize,
}

/// The plan as this check reads it.
#[derive(Clone, Copy, Debug)]
pub struct PlanView<'a> {
    pub workers: &'a [WorkerView],
    pub routes: &'a [RouteView],
    /// `Plan::publication.len()`.
    pub publication: usize,
}

/// What the host probed about the device, before launch. Values are the caller's; this module
/// queries nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// How many workers can be resident at once (co-residency or a cooperative grid).
    pub resident_workers: usize,
    /// Bytes available to hold every route's frames, all slots of all routes.
    pub route_bytes: usize,
    /// Bytes available to hold the publication.
    pub publication_bytes: usize,
}

/// A refusal before any worker starts. Same shape as `launch::Refused::Backend`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Backend { reason: &'static str },
}

/// Check the plan against the limits. Accepts exactly when every condition holds:
///
/// 1. every worker fits in the resident grid (`workers.len() <= resident_workers`);
/// 2. no worker's captures own memory (`captures_drop_free`);
/// 3. the frames of all routes fit (`sum of cap * size <= route_bytes`), computed without overflow;
/// 4. the publication fits (`publication <= publication_bytes`).
///
/// Gaps, which this check does not close: (a) state created inside a worker body, including
/// `Vec`s, `parallel!` slots and `concurrent!` cells, is not in the plan, so a plan that accepts can
/// still run a non-representable state; (b) a captured reference or raw pointer passes (2); (c)
/// `barrier()` returns `Arc<[u8]>`, which the device cannot hold, and no plan field records that a
/// body calls it. Closing (a)-(c) needs the driver, which sees the bodies, or a typed registration
/// that records more than `needs_drop`.
pub fn representable(plan: &PlanView<'_>, limits: &Limits) -> Result<(), Refusal> {
    if plan.workers.len() > limits.resident_workers {
        return Err(Refusal::Backend { reason: "more workers than the grid can keep resident at once" });
    }
    if plan.workers.iter().any(|w| !w.captures_drop_free) {
        return Err(Refusal::Backend {
            reason: "a worker captures a value that owns memory; the device has no allocator",
        });
    }
    let mut bytes = 0_usize;
    for r in plan.routes {
        let Some(b) = r.cap.checked_mul(r.size).and_then(|b| bytes.checked_add(b)) else {
            return Err(Refusal::Backend { reason: "route storage overflows; the plan is too large to hold" });
        };
        bytes = b;
    }
    if bytes > limits.route_bytes {
        return Err(Refusal::Backend { reason: "the routes' frames do not fit in device memory" });
    }
    if plan.publication > limits.publication_bytes {
        return Err(Refusal::Backend { reason: "the publication does not fit in the device's publication buffer" });
    }
    Ok(())
}

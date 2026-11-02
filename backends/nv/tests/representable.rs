//! The representability check (src/representable.rs): accepted plans, refused plans, and the boundary
//! of each condition. Host-only; no GPU is involved.
use trame_nv::representable::{Limits, PlanView, Refusal, RouteView, WorkerView, representable};

const W: WorkerView = WorkerView { captures_drop_free: true };
const HEAP: WorkerView = WorkerView { captures_drop_free: false };

fn limits() -> Limits {
    Limits { resident_workers: 4, route_bytes: 1024, publication_bytes: 64 }
}

fn plan<'a>(workers: &'a [WorkerView], routes: &'a [RouteView], publication: usize) -> PlanView<'a> {
    PlanView { workers, routes, publication }
}

fn backend(r: Result<(), Refusal>) -> &'static str {
    match r {
        Ok(()) => panic!("expected a refusal, got an acceptance"),
        Err(Refusal::Backend { reason }) => reason,
    }
}

#[test]
fn accepts_a_plan_within_every_limit() {
    let ws = [W, W];
    let rs = [RouteView { cap: 4, size: 16 }, RouteView { cap: 1, size: 8 }];
    assert_eq!(representable(&plan(&ws, &rs, 0), &limits()), Ok(()));
}

#[test]
fn accepts_the_empty_plan() {
    assert_eq!(representable(&plan(&[], &[], 0), &limits()), Ok(()));
}

#[test]
fn accepts_each_condition_at_its_limit() {
    let l = limits();
    let ws = [W; 4];
    let rs = [RouteView { cap: 64, size: 16 }]; // exactly 1024 bytes
    assert_eq!(representable(&plan(&ws, &rs, 64), &l), Ok(()));
}

#[test]
fn refuses_one_worker_too_many() {
    let ws = [W; 5];
    let r = representable(&plan(&ws, &[], 0), &limits());
    assert!(backend(r).contains("resident"));
}

#[test]
fn refuses_a_worker_whose_captures_own_memory() {
    let ws = [W, HEAP];
    let r = representable(&plan(&ws, &[], 0), &limits());
    assert!(backend(r).contains("owns memory"));
}

#[test]
fn refuses_route_storage_one_byte_over() {
    let ws = [W];
    let rs = [RouteView { cap: 64, size: 16 }, RouteView { cap: 1, size: 1 }]; // 1025 bytes
    assert!(backend(representable(&plan(&ws, &rs, 0), &limits())).contains("routes"));
}

#[test]
fn refuses_route_storage_that_overflows() {
    let ws = [W];
    let rs = [RouteView { cap: usize::MAX, size: 2 }];
    assert!(backend(representable(&plan(&ws, &rs, 0), &limits())).contains("overflows"));
}

#[test]
fn refuses_a_publication_one_byte_over() {
    let ws = [W];
    assert!(backend(representable(&plan(&ws, &[], 65), &limits())).contains("publication"));
}

/// The gap, as a test: the plan of `programs/post_parallel_heap.rs` (one worker, no captures, one route,
/// no publication) is accepted, though the worker's body allocates a `Vec` of slots. The check cannot see
/// that, so acceptance here is not evidence the plan runs on the device.
#[test]
fn accepts_the_heap_slot_program_because_the_plan_cannot_see_its_body() {
    let ws = [W];
    let rs = [RouteView { cap: 4, size: 16 }];
    assert_eq!(representable(&plan(&ws, &rs, 0), &limits()), Ok(()));
}

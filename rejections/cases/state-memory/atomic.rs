// spec: CONTRACT.md, M3 (no interior mutability): an atomic changes behind the field lock. It is Send and Sync, so rustc accepts it.
use std::sync::atomic::AtomicU64;

#[trame::state(steps(step))]
struct Hold {
    n: AtomicU64, //~ ERROR: [state-memory] Hold.n: std::sync::atomic::Atomic<u64> holds an atomic
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

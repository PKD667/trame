// spec: CONTRACT.md, M3 (no interior mutability): an atomic changes behind the field lock. It is Send and Sync, so only Plain refuses it.
use std::sync::atomic::AtomicU64;

#[trame::state(steps(step))]
struct Hold {
    n: AtomicU64, //~ ERROR: cannot be held by a trame state, pool or error
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

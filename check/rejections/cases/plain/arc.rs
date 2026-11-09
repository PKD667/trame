// spec: CONTRACT.md, M2 (no process meaning): an Arc is a reference count in one process. It is Send and Sync, so only Plain refuses it.
use std::sync::Arc;

#[trame::state(steps(step))]
struct Hold {
    shared: Arc<u32>, //~ ERROR: cannot be held by a trame state, pool or error
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

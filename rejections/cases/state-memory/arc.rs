// spec: CONTRACT.md, M2 (no process meaning): an Arc is a reference count in one process. It is Send and Sync, so rustc accepts it.
use std::sync::Arc;

#[trame::state(steps(step))]
struct Hold {
    shared: Arc<u32>, //~ ERROR: [state-memory] Hold.shared: std::sync::Arc<u32> holds an Arc (a reference count in one process)
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

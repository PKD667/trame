// spec: backend.rs M2 (no process meaning): an Arc's Weak, as Arc.
use std::sync::Weak;

#[trame::state(steps(step))]
struct Hold {
    shared: Weak<u32>, //~ ERROR: [state-memory] Hold.shared: std::sync::Weak<u32> holds an Arc (a reference count in one process)
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

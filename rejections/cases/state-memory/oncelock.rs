// spec: backend.rs M3 (no interior mutability): an OnceLock is set through a shared reference: interior mutability, Send and Sync.
use std::sync::OnceLock;

#[trame::state(steps(step))]
struct Hold {
    m: OnceLock<u32>, //~ ERROR: [state-memory] Hold.m: std::sync::OnceLock<u32> holds a lock or a shared cell
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

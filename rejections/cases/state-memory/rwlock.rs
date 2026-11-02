// spec: CONTRACT.md, M3 (no interior mutability): an RwLock, as Mutex.
use std::sync::RwLock;

#[trame::state(steps(step))]
struct Hold {
    m: RwLock<Vec<u32>>, //~ ERROR: [state-memory] Hold.m: std::sync::RwLock<std::vec::Vec<u32>> holds a lock or a shared cell
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

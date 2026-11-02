// spec: backend.rs M3 (no interior mutability): a Mutex is a lock inside the state: field locks are the only synchronisation.
use std::sync::Mutex;

#[trame::state(steps(step))]
struct Hold {
    m: Mutex<u32>, //~ ERROR: [state-memory] Hold.m: std::sync::Mutex<u32> holds a lock or a shared cell
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

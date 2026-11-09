// spec: CONTRACT.md, M3 (no interior mutability): an RwLock, as Mutex.
use std::sync::RwLock;

#[trame::state(steps(step))]
struct Hold {
    m: RwLock<Vec<u32>>, //~ ERROR: cannot be held by a trame state, pool or error
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

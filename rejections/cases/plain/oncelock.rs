// spec: CONTRACT.md, M3 (no interior mutability): an OnceLock is set through a shared reference: interior mutability, Send and Sync.
use std::sync::OnceLock;

#[trame::state(steps(step))]
struct Hold {
    m: OnceLock<u32>, //~ ERROR: cannot be held by a trame state, pool or error
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

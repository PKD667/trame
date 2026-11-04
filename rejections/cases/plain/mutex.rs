// spec: CONTRACT.md, M3 (no interior mutability): a Mutex is a lock inside the state: field locks are the only synchronisation.
use std::sync::Mutex;

#[trame::state(steps(step))]
struct Hold {
    m: Mutex<u32>, //~ ERROR: cannot be held by a trame state or pool
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

// spec: CONTRACT.md, M2 (no process meaning): an Instant is read from one process's clock.
use std::time::Instant;

#[trame::state(steps(step))]
struct Hold {
    t: Instant, //~ ERROR: cannot be held by a trame state, pool or error
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

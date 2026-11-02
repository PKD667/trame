// spec: backend.rs M2 (no process meaning): an Instant is read from one process's clock.
use std::time::Instant;

#[trame::state(steps(step))]
struct Hold {
    t: Instant, //~ ERROR: [state-memory] Hold.t: std::time::Instant holds an instant of one process's clock
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

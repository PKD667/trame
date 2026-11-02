// spec: backend.rs M3 (no interior mutability): the pool is checked as a state is: a Cell under a user pool type that promises Sync.
use std::cell::Cell;

struct Table {
    hits: Cell<u64>, //~ ERROR: [state-memory] Table.hits: std::cell::Cell<u64> holds interior mutability (backend.rs M3)
}

// SAFETY: for the case only.
unsafe impl Sync for Table {}

#[trame::state(steps(step), pool = Table)]
struct Hold {
    n: u64,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

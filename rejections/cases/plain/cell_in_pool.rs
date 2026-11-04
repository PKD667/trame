// spec: CONTRACT.md, M3 (no interior mutability): the pool is checked as a state is, by the derive on its type: a Cell under a user pool type that promises Sync.
use std::cell::Cell;

#[derive(trame::Plain)]
struct Table {
    hits: Cell<u64>, //~ ERROR: cannot be held by a trame state or pool
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

// spec: CONTRACT.md, M3 (no interior mutability): the cell is two structs down, under a user type that promises Sync: each user type derives Plain, and the derive
// checks its own fields.
use std::cell::Cell;

#[derive(trame::Plain)]
struct Counter {
    c: Cell<u32>, //~ ERROR: cannot be held by a trame state or pool
}

// SAFETY: for the case only; Plain does not follow from Sync.
unsafe impl Sync for Counter {}

#[derive(trame::Plain)]
struct Outer {
    inner: Counter,
}

#[trame::state(steps(step))]
struct Hold {
    outer: Outer,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

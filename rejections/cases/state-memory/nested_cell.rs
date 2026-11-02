// spec: CONTRACT.md, M3 (no interior mutability): the cell is two structs down, under a user type that promises Sync: the check follows the layout, not the
// auto traits.
use std::cell::Cell;

struct Counter {
    c: Cell<u32>,
}

// SAFETY: for the case only; the driver reads the layout, not this promise.
unsafe impl Sync for Counter {}

struct Outer {
    inner: Counter,
}

#[trame::state(steps(step))]
struct Hold {
    outer: Outer, //~ ERROR: [state-memory] Hold.outer.inner.c: std::cell::Cell<u32> holds interior mutability (CONTRACT.md, M3)
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

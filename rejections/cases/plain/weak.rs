// spec: CONTRACT.md, M2 (no process meaning): an Arc's Weak, as Arc.
use std::sync::Weak;

#[trame::state(steps(step))]
struct Hold {
    shared: Weak<u32>, //~ ERROR: cannot be held by a trame state, pool or error
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

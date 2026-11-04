// spec: CONTRACT.md, M2 (no process meaning): an Rc under a user type with an unsafe Send and Sync.
use std::rc::Rc;

#[derive(trame::Plain)]
struct Shared(Rc<u32>); //~ ERROR: cannot be held by a trame state or pool

// SAFETY: for the case only.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

#[trame::state(steps(step))]
struct Hold {
    s: (u8, Shared),
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

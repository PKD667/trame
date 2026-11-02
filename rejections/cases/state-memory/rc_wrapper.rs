// spec: CONTRACT.md, M2 (no process meaning): an Rc under a user type with an unsafe Send and Sync.
use std::rc::Rc;

struct Shared(Rc<u32>);

// SAFETY: for the case only.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

#[trame::state(steps(step))]
struct Hold {
    s: (u8, Shared), //~ ERROR: [state-memory] Hold.s.1.0: std::rc::Rc<u32> holds an Rc (a reference count in one process)
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

// spec: CONTRACT.md, M3 (no interior mutability): a Vec's element type is read: the refusal names the field of the state and the type found.
use std::cell::RefCell;

#[derive(trame::Plain)]
struct Slot(RefCell<u8>); //~ ERROR: cannot be held by a trame state or pool

// SAFETY: for the case only.
unsafe impl Sync for Slot {}

#[trame::state(steps(step))]
struct Hold {
    slots: Vec<Slot>,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

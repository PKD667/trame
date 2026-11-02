// spec: backend.rs M3 (no interior mutability): a Vec's element type is read: the refusal names the field of the state and the type found.
use std::cell::RefCell;

struct Slot(RefCell<u8>);

// SAFETY: for the case only.
unsafe impl Sync for Slot {}

#[trame::state(steps(step))]
struct Hold {
    slots: Vec<Slot>, //~ ERROR: [state-memory] Hold.slots.0: std::cell::RefCell<u8> holds interior mutability (backend.rs M3)
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

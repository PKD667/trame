// rustc refuses a Cell field before the driver (State: Sync). See nested_cell.rs for the layout walk.
use std::cell::Cell;

#[trame::state(steps(step))] //~ ERROR: `std::cell::Cell<u32>` cannot be shared between threads safely
struct Hold {
    //~^ ERROR: `std::cell::Cell<u32>` cannot be shared between threads safely
    n: Cell<u32>,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

// spec: backend.rs M2. rustc refuses an Rc field before the driver does: the state macro needs `Send + Sync` of the state
// (worker.rs, State). The driver's check is the second line, for what rustc accepts (arc.rs, rc_wrapper.rs).
use std::rc::Rc;

#[trame::state(steps(step))] //~ ERROR: `std::rc::Rc<u32>` cannot be sent between threads safely
//~^ ERROR: `std::rc::Rc<u32>` cannot be shared between threads safely
struct Hold {
    //~^ ERROR: `std::rc::Rc<u32>` cannot be sent between threads safely
    //~| ERROR: `std::rc::Rc<u32>` cannot be shared between threads safely
    shared: Rc<u32>,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

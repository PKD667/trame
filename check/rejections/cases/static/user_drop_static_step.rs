// spec: Requirement (All levels): no unsafe and no globals. Drop glue counts as a call. q.v = None drops the X, whose Drop
// writes the static mut COUNT inside an unsafe block.
// Refused [unsafe] step (two): the unsafe block, and the dereference of the raw pointer that the static access makes.
// Refused [static] step: COUNT is a global; only consts are allowed.
static mut COUNT: u32 = 0;

#[derive(trame::Plain)]
struct X;

impl Drop for X {
    fn drop(&mut self) {
        unsafe { COUNT += 1; }
        //~^ ERROR: [unsafe] step
        //~| ERROR: [static] step
        //~| ERROR: [unsafe] step
    }
}

#[trame::message]
struct M {
    n: u32,
}

#[derive(trame::Plain)]
struct Q {
    v: Option<X>,
}

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    q.v = None;
    trame::Status::Done
}

fn main() {}

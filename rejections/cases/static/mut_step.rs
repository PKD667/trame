// spec: Requirement (All levels): no globals, and no mutable globals (static mut is a global with unsafe
// access). A step reads one inside an unsafe block.
static mut COUNT: u32 = 7;

#[trame::message]
struct Job {
    id: u32,
}

struct Queue {
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    q: Queue,
}

#[trame::step(S, receives = Job)]
fn step(q: &mut Queue) -> trame::Status {
    q.n = unsafe { COUNT };
    //~^ ERROR: [unsafe] step
    //~| ERROR: [static] step
    //~| ERROR: [unsafe] step
    trame::Status::Done
}

fn main() {}

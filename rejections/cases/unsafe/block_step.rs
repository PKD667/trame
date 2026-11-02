// spec: Requirement (All levels): no unsafe. A step reads through a raw pointer in an unsafe block.
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
    let p = &q.n as *const u32;
    q.n = unsafe { *p };
    //~^ ERROR: [unsafe] step
    //~| ERROR: [unsafe] step
    trame::Status::Done
}

fn main() {}

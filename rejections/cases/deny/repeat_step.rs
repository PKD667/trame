// spec: Definition (Counted loop), Requirement (Step): a step's loop is counted. core::iter::repeat is an infinite
// source: iterating it in a step has no bound. (It is denied at Item in A_I.txt, and must be at Step too.)
#[trame::message]
struct Job {
    id: u32,
}

#[derive(trame::Plain)]
struct Queue {
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    q: Queue,
}

#[trame::step(S, receives = Job)]
fn step(q: &mut Queue) -> trame::Status {
    core::iter::repeat(1u32).for_each(|x| q.n += x);
    //~^ ERROR: [deny] step
    //~| ERROR: [deny] step
    //~| ERROR: [deny] step
    trame::Status::Done
}

fn main() {}

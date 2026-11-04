//@check-pass
// spec: Requirement (All levels): no globals. This is the pair of read_step.rs: a const is a value, not a
// global, so the same read is accepted.
const COUNT: u32 = 7;

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
    q.n = COUNT;
    trame::Status::Done
}

fn main() {}

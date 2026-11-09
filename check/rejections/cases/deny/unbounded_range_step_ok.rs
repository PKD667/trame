//@check-pass
// spec: Definition (Counted loop). This is the pair of unbounded_range_step.rs: the same iteration over a
// bounded range is counted, and accepted.
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
    (0u32..8).for_each(|_| q.n += 1);
    trame::Status::Done
}

fn main() {}

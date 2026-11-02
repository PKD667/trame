//@check-pass
// spec: Definition (Counted loop). This is the pair of while_step.rs: the same three iterations written as a
// `for` over a range are counted, and accepted.
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
    let mut i = 0u32;
    for _ in 0..3 {
        i += 1;
    }
    q.n = i;
    trame::Status::Done
}

fn main() {}

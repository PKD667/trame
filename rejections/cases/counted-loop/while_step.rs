// spec: Definition (Counted loop), Requirement (Step): every loop of a step is counted. A while loop is not.
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
    let mut i = 0u32;
    while i < 3 { //~ ERROR: [counted-loop] step
        i += 1;
    }
    q.n = i;
    trame::Status::Done
}

fn main() {}

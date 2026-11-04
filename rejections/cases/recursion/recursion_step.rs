// spec: Requirement (Step): a step does not recurse. A step calls a function that calls itself.
fn down(n: u32) -> u32 {
    if n == 0 { 0 } else { down(n - 1) } //~ ERROR: [recursion] step
}

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
    q.n = down(3);
    trame::Status::Done
}

fn main() {}

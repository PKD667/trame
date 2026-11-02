// spec: Requirement (Step): a step calls only items of A_C. A closure passed to for_each is called by core,
// so the closure's std call is refused at the step, as if it were written in the step.
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
    [1u32, 2].iter().for_each(|_| std::println!("x")); //~ ERROR: [std] step
    q.n += 1;
    trame::Status::Done
}

fn main() {}

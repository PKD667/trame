//@check-pass
// spec: Definition (Counted loop). This is the pair of vec_iter_mutated_step.rs: a `for` over a slice that the
// body only reads is counted, and accepted. The Vec is a field of the state: a step does not make one.
#[trame::message]
struct Job {
    id: u32,
}

#[trame::state(steps(step))]
struct S {
    v: Vec<u32>,
    n: u32,
}

#[trame::step(S, receives = Job)]
fn step(v: &Vec<u32>, n: &mut u32) -> trame::Status {
    let mut sum = 0u32;
    for x in v.iter() {
        sum += *x;
    }
    *n = sum;
    trame::Status::Done
}

fn main() {}

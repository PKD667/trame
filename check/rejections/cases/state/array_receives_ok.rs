//@check-pass
// spec: backend.rs, Scheduling: a step's `receives` takes any message type, an array included, since an array of a
// message struct is one (Requirement (Messages)). The step rewrites a Vec field in place, which it may do (M4).
#[trame::message]
struct Spike {
    neuron: u32,
}

#[trame::state(steps(step))]
struct S {
    p: Vec<u32>,
}

#[trame::step(S, receives = [Spike; 4])]
fn step(p: &mut Vec<u32>) -> trame::Status {
    p[0] = 1;
    trame::Status::Done
}

fn main() {}

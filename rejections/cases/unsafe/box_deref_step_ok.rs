//@check-pass
// spec: Requirement (All levels) forbids unsafe code, not Box. Box<u32> is an owning pointer; `*b` is a safe read
// and write through it, and does not allocate. The Box is allocated by the builder.
#[trame::message]
struct M {
    n: u32,
}

struct Q {
    hand: Box<u32>,
}

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    let x = *q.hand + 1;
    *q.hand = x;
    trame::Status::Done
}

fn main() {}

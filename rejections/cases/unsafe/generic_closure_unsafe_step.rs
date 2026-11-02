// spec: Requirement (All levels): no unsafe. The closure's body holds unsafe code; it is passed to a user generic
// function, which calls it. The closure is user code.
fn apply<F: Fn(u32) -> u32>(f: F, x: u32) -> u32 {
    f(x)
}

#[trame::message]
struct M {
    n: u32,
}

struct Q {
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    q.n = apply(|k| { let p = &k as *const u32; unsafe { *p } }, q.n);
    //~^ ERROR: [unsafe] step
    //~| ERROR: [unsafe] step
    trame::Status::Done
}

fn main() {}

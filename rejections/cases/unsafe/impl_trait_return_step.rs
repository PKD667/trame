// spec: Requirement (All levels): no unsafe. The returned impl Fn hides a closure with unsafe code; the step calls it.
fn make(k: u32) -> impl Fn(u32) -> u32 {
    move |x| { let p = &x as *const u32; unsafe { *p + k } }
    //~^ ERROR: [unsafe] step
    //~| ERROR: [unsafe] step
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
    q.n = make(1)(q.n);
    trame::Status::Done
}

fn main() {}

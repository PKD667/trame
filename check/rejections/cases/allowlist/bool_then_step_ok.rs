//@check-pass
// spec: Requirement (Step) allows core calls in A_C; bool::then with a pure closure allocates nothing and calls no
// std. A_C lists no core::bool.
#[trame::message]
struct M {
    n: u32,
}

#[derive(trame::Plain)]
struct Q {
    n: u32,
    tag: Option<u32>,
}

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    q.tag = (q.n > 0).then(|| q.n);
    trame::Status::Done
}

fn main() {}

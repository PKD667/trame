//@check-pass
// spec: Definition (Counted loop): a for over a slice, reversed, that the body does not mutate, is counted.
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
    let v = [1_u32, 2, 3];
    for x in v.iter().rev() {
        q.n += *x;
    }
    trame::Status::Done
}

fn main() {}

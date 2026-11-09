//@check-pass
// spec: Definition (Counted loop): a for over a range, or over one of these reversed, that the body does not mutate.
// (0..4).rev() is a range, reversed, and the body only reads, so it is counted: accepted.
#[trame::message]
struct M {
    n: u32,
}

#[derive(trame::Plain)]
struct Q {
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    for i in (0..4_u32).rev() {
        q.n += i;
    }
    trame::Status::Done
}

fn main() {}

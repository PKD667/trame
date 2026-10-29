//@check-pass
// spec: Definition (Counted loop): a for over a range, or over one of these reversed, that the body does not mutate.
// (0..4).rev() is a range, reversed, and the body only reads, so it is counted: accepted.
#[trame::message]
struct M {
    n: u32,
}

struct Q {
    n: u32,
}

struct P;

#[trame::process(receives = M)]
impl P {
    fn step(&mut self, q: &mut Q) -> trame::Status {
        for i in (0..4_u32).rev() {
            q.n += i;
        }
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { n: 0 };
    let _ = trame::concurrent!(P; q);
}

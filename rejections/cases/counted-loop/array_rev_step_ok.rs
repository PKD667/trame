//@check-pass
// spec: Definition (Counted loop): a for over an array, reversed, that the body does not mutate, is counted.
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
        for x in [1_u32, 2, 3].into_iter().rev() {
            q.n += x;
        }
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { n: 0 };
    let _ = trame::concurrent!(P; q);
}

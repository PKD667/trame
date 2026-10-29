//@check-pass
// spec: Definition (Counted loop): a for over a slice, reversed, that the body does not mutate, is counted.
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
        let v = [1_u32, 2, 3];
        for x in v.iter().rev() {
            q.n += *x;
        }
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { n: 0 };
    let _ = trame::concurrent!(P; q);
}

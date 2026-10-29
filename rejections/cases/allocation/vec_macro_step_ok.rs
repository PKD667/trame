//@check-pass
// spec: Requirement (Step): a step may allocate. vec![] lowers to a box expression.
#[trame::message]
struct M {
    n: u32,
}

struct Q {
    n: u32,
    out: Vec<u32>,
}

struct P;

#[trame::process(receives = M)]
impl P {
    fn step(&mut self, q: &mut Q) -> trame::Status {
        q.out = vec![q.n];
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { n: 1, out: Vec::new() };
    let _ = trame::concurrent!(P; q);
}

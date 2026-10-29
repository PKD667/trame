//@check-pass
// spec: Requirement (Step) allows core calls in A_C; bool::then with a pure closure allocates nothing and calls no
// std. A_C lists no core::bool.
#[trame::message]
struct M {
    n: u32,
}

struct Q {
    n: u32,
    tag: Option<u32>,
}

struct P;

#[trame::process(receives = M)]
impl P {
    fn step(&mut self, q: &mut Q) -> trame::Status {
        q.tag = (q.n > 0).then(|| q.n);
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { n: 1, tag: None };
    let _ = trame::concurrent!(P; q);
}

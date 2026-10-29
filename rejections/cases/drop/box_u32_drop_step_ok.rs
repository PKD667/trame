//@check-pass
// spec: Requirement (All levels): drop glue counts as a call. Box<u32> has no Drop impl of its own: its glue is
// library code (Box's), judged by its list entry, and it calls nothing forbidden at drop. Its raw pointer derefs are
// the glue's own, not [unsafe]. A step may allocate (Requirement (Step)), so the Box is accepted.
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
        let b = Box::new(q.n);
        q.n = *b + 1;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { n: 1 };
    let _ = trame::concurrent!(P; q);
}

//@check-pass
// spec: Requirement (All levels) forbids unsafe code, not Box. Box<u32> is an owning pointer; `*b` is a safe read.
#[trame::message]
struct M {
    n: u32,
}

struct Q {
    hand: Box<u32>,
}

struct P;

#[trame::process(receives = M)]
impl P {
    fn step(&mut self, q: &mut Q) -> trame::Status {
        let x = *q.hand + 1;
        q.hand = Box::new(x);
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { hand: Box::new(1) };
    let _ = trame::concurrent!(P; q);
}

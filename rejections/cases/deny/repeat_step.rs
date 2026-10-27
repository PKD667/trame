//! expect: refused [deny] step
// spec: Definition (Counted loop), Requirement (Step): a step's loop is counted. core::iter::repeat is an infinite
// source: iterating it in a step has no bound. (It is denied at Item in A_I.txt, and must be at Step too.)
#[trame::message]
struct Job {
    id: u32,
}

struct Queue {
    n: u32,
}

struct P;

#[trame::process(receives = Job)]
impl P {
    fn step(&mut self, q: &mut Queue) -> trame::Status {
        core::iter::repeat(1u32).for_each(|x| q.n += x);
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

//! expect: accepted
// spec: Requirement (All levels): no globals. This is the pair of read_step.rs: a const is a value, not a
// global, so the same read is accepted.
const COUNT: u32 = 7;

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
        q.n = COUNT;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

//! expect: accepted
// spec: Definition (Counted loop). This is the pair of unbounded_range_step.rs: the same iteration over a
// bounded range is counted, and accepted.
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
        (0u32..8).for_each(|_| q.n += 1);
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

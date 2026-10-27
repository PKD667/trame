//! expect: accepted
//! known: a step may iterate a slice (spec.md, Definition (Counted loop)), but core::slice::Iter reads raw pointers, so the driver refuses it as unsafe (finding C1).
// spec: Definition (Counted loop). This is the pair of vec_iter_mutated_step.rs: a `for` over a slice that the
// body only reads is counted, and accepted.
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
        let v: Vec<u32> = vec![1, 2];
        let mut sum = 0u32;
        for x in v.iter() {
            sum += *x;
        }
        q.n = sum;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

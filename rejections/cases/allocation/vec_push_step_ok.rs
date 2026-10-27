//! expect: accepted
// spec: Requirement (Step): a step may allocate. This is the pair of vec_push_item.rs: the same push, in a step.
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
        let mut v: Vec<u32> = Vec::new();
        v.push(1);
        q.n = v.len() as u32;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

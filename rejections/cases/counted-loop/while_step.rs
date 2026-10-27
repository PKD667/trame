//! expect: refused [counted-loop] step
// spec: Definition (Counted loop), Requirement (Step): every loop of a step is counted. A while loop is not.
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
        let mut i = 0u32;
        while i < 3 {
            i += 1;
        }
        q.n = i;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

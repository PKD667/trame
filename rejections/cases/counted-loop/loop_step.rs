//! expect: refused [counted-loop] step
// spec: Definition (Counted loop), Requirement (Step): `loop` has no bound in its header. It is not counted,
// even though this one breaks.
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
        loop {
            i += 1;
            if i == 3 {
                break;
            }
        }
        q.n = i;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

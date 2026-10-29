// spec: Definition (Counted loop), Requirement (Step): a step's loop is counted, so it terminates. `0..` is an
// infinite range: a step that iterates over it has no bound.
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
        (0u32..).for_each(|_| q.n += 1);
        //~^ ERROR: [deny] step
        //~| ERROR: [deny] step
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

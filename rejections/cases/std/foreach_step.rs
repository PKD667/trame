//! expect: refused [std] step
// spec: Requirement (Step): a step calls only items of A_C. A closure passed to for_each is called by core,
// so the closure's std call is refused at the step, as if it were written in the step.
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
        [1u32, 2].iter().for_each(|_| std::println!("x"));
        q.n += 1;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

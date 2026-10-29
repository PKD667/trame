// spec: Requirement (Step): a step does not recurse. A step calls a function that calls itself.
fn down(n: u32) -> u32 {
    if n == 0 { 0 } else { down(n - 1) } //~ ERROR: [recursion] step
}

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
        q.n = down(3);
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

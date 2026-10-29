// spec: Requirement (Step): a step does not recurse, directly or through other functions. Two functions that
// call each other form a cycle.
fn even(n: u32) -> bool {
    if n == 0 { true } else { odd(n - 1) }
}

fn odd(n: u32) -> bool {
    if n == 0 { false } else { even(n - 1) } //~ ERROR: [recursion] step
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
        q.n = even(4) as u32;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

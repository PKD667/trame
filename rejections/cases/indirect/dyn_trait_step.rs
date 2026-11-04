// spec: Requirement (All levels): no indirect calls. A step calls a method through a trait object.
trait Scale {
    fn scale(&self, x: u32) -> u32;
}

struct Double;

impl Scale for Double {
    fn scale(&self, x: u32) -> u32 {
        x * 2
    }
}

#[trame::message]
struct Job {
    id: u32,
}

#[derive(trame::Plain)]
struct Queue {
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    q: Queue,
}

#[trame::step(S, receives = Job)]
fn step(q: &mut Queue) -> trame::Status {
    let s: &dyn Scale = &Double;
    q.n = s.scale(q.n); //~ ERROR: [indirect] step
    trame::Status::Done
}

fn main() {}

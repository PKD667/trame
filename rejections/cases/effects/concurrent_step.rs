// spec: Requirement (Step), Definition (Effect sets): Sigma_C has no concurrent. A step may not start a
// concurrent! of other processes: that is a worker's effect.
#[trame::message]
struct Job {
    id: u32,
}

struct Queue {
    n: u32,
}

struct Inner;

#[trame::process(receives = Job)]
impl Inner {
    fn step(&mut self, q: &mut Queue) -> trame::Status {
        q.n += 1;
        trame::Status::Done
    }
}

struct Outer;

#[trame::process]
impl Outer {
    fn step(&mut self, q: &mut Queue) -> trame::Status {
        let mut local = Queue { n: 0 };
        let _ = trame::concurrent!(Inner; local); //~ ERROR: [effects] step
        q.n = local.n;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(Outer; q);
}

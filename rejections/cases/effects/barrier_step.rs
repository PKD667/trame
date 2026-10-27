//! expect: refused [allow-list] step, [effects] step
// spec: Requirement (Step), Definition (Effect sets): Sigma_C has no barrier. The barrier involves every worker,
// so a step may not wait on it. Only a worker may.
#[trame::message]
struct Msg {
    n: u32,
}

struct Queue {
    n: u32,
}

struct P;

#[trame::process(receives = Msg)]
impl P {
    fn step(&mut self, q: &mut Queue) -> trame::Status {
        let _ = trame::barrier();
        q.n += 1;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

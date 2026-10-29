// spec: Requirement (Step), Definition (Effect sets): Sigma_C has no barrier. The barrier involves every worker, so a step
// may not wait on it. Only a worker may.
// Refused [effects] step: the call to trame::barrier is an effect that a step may not perform.
// Refused [allow-list] step: the Arc that barrier returns is dropped at once, and Arc::drop is not in A_C (driver/lists/A_C.txt).
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
        //~^ ERROR: [effects] step
        //~| ERROR: [allow-list] step
        q.n += 1;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

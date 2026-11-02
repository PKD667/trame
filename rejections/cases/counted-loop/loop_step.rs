// spec: Definition (Counted loop), Requirement (Step): `loop` has no bound in its header. It is not counted,
// even though this one breaks.
#[trame::message]
struct Job {
    id: u32,
}

struct Queue {
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    q: Queue,
}

#[trame::step(S, receives = Job)]
fn step(q: &mut Queue) -> trame::Status {
    let mut i = 0u32;
    loop {
        i += 1; //~ ERROR: [counted-loop] step
        if i == 3 {
            break;
        }
    }
    q.n = i;
    trame::Status::Done
}

fn main() {}

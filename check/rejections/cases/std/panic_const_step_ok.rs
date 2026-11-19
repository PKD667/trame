//@check-pass
// spec: Definition (Abort): a panic aborts the launch; it is not an effect and not std. A step panics with a
// constant message; both are accepted.
#[trame::message]
struct Job {
    id: u32,
}

#[derive(trame::Plain)]
struct Queue {
    n: u32,
    cap: u32,
}

#[trame::state(steps(step))]
struct S {
    q: Queue,
}

fn push(q: &mut Queue, n: u32) {
    if q.n >= q.cap {
        panic!("full");
    }
    q.n = n;
}

#[trame::step(S, receives = Job)]
fn step(q: &mut Queue) -> trame::Status {
    push(q, 1);
    trame::Status::Done
}

fn main() {}

//@check-pass
// spec: Definition (Abort): a panic aborts the launch; it is not an effect and not std. A step panics with a formatted
// message and a runtime argument (a constant one is folded by MIR inlining and proves nothing). nerve, point 4: the
// format_args! expansion calls core's unsafe Arguments::new, which is not an unsafe act of the program: accepted.
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

fn push(q: &mut Queue, n: u32) {
    if q.n >= 100 {
        panic!("{}", n);
    }
    q.n = n;
}

#[trame::step(S, receives = Job)]
fn step(q: &mut Queue) -> trame::Status {
    let n = q.n + 1;
    push(q, n);
    trame::Status::Done
}

fn main() {}

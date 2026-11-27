//@check-pass
// spec: Definition (Abort): a panic is not an effect and not std. bisected: a free fn, send then panic with a u64.
#[trame::message]
struct Msg {
    n: u32,
}

#[derive(trame::Plain)]
struct Queue {
    n: u32,
    dropped: u64,
}

#[trame::state(steps(step))]
struct S {
    q: Queue,
}

fn put(lost: u64) {
    if trame::send(trame::Leader, Msg { n: 1 }).is_err() {
        panic!("a route is full: {}", lost);
    }
}

#[trame::step(S, receives = Msg)]
fn step(q: &mut Queue) -> trame::Status {
    put(q.dropped);
    trame::Status::Done
}

fn main() {}

//@check-pass
// spec: Definition (Abort): a panic is not an effect and not std. bisected: a generic free fn (M: Message, impl Into<End>).
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

fn put<M: trame::Message>(to: impl Into<trame::End>, msg: M, lost: u32) {
    if trame::send(to, msg).is_err() {
        panic!("a route is full: {}", lost);
    }
}

#[trame::step(S, receives = Msg)]
fn step(q: &mut Queue) -> trame::Status {
    put(trame::Leader, Msg { n: 1 }, q.n);
    trame::Status::Done
}

fn main() {}

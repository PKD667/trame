//@check-pass
// spec: Definition (Abort): a panic is not an effect and not std. nerve point 4, bisected: the whole shape: generic method, track_caller, u64 field.
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

impl Queue {
    #[track_caller]
    fn put<M: trame::Message>(&mut self, to: impl Into<trame::End>, msg: M, lost: u64) {
        if trame::send(to, msg).is_err() {
            panic!("a route is full: {}", lost);
        }
    }
}

#[trame::step(S, receives = Msg)]
fn step(q: &mut Queue) -> trame::Status {
    q.put(trame::Leader, Msg { n: 1 }, q.dropped);
    trame::Status::Done
}

fn main() {}

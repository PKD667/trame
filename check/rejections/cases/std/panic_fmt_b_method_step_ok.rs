//@check-pass
// spec: Definition (Abort): a panic is not an effect and not std. nerve point 4, bisected: a &mut self method, not generic, u32 arg.
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
    fn put(&mut self, lost: u32) {
        if trame::send(trame::Leader, Msg { n: 1 }).is_err() {
            panic!("a route is full: {}", lost);
        }
    }
}

#[trame::step(S, receives = Msg)]
fn step(q: &mut Queue) -> trame::Status {
    q.put(q.n);
    trame::Status::Done
}

fn main() {}

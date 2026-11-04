//@check-pass
// spec: Definition (Effect sets): Sigma_C contains send. This is the pair of send_item.rs: the same send, in a step.
#[trame::message]
struct Msg {
    n: u32,
}

#[derive(trame::Plain)]
struct Queue {
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    q: Queue,
}

#[trame::step(S, receives = Msg)]
fn step(q: &mut Queue) -> trame::Status {
    let _ = trame::send(trame::LEADER, Msg { n: q.n });
    trame::Status::Done
}

fn main() {}

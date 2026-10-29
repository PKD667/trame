//@check-pass
// spec: Definition (Effect sets): Sigma_C contains send. This is the pair of send_item.rs: the same send, in a step.
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
        let _ = trame::send(trame::LEADER, Msg { n: q.n });
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

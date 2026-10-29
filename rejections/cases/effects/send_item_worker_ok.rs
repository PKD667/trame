//@check-pass
// spec: Definition (Effect sets): Sigma_W contains send. The worker pair of send_item.rs: a worker may send. send
// takes a message type (io.rs: `send<M: Message>`), so the case declares one.
#[trame::message]
struct Msg {
    n: u32,
}

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = trame::send(trame::LEADER, Msg { n: 1 });
    });
}

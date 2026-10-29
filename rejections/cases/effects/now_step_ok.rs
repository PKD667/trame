//@check-pass
// spec: Definition (Effect sets): Sigma_C contains now. This is the pair of now_item.rs: the same clock read, in a step.
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
        let _ = trame::now();
        q.n += 1;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

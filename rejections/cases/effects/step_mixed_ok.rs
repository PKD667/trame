//@check-pass
// spec: Requirement (Step), Definition (Effect sets): a step may allocate with a fixed capacity, runs counted
// loops, and performs Sigma_C effects (send, now). math::f32::exp is trame's own function, trusted at every level.
// This is the old fixture step_ok, against the real surface.
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
        let mut buffer: Vec<u32> = Vec::with_capacity(4);
        let mut sum = [0u32; 4];
        for i in 0..4usize {
            sum[i] = i as u32;
            buffer.push(sum[i]);
        }
        let _ = trame::send(trame::LEADER, Msg { n: q.n });
        let _ = trame::now();
        q.n = trame::math::f32::exp(1.0) as u32 + sum[3] + buffer.len() as u32;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

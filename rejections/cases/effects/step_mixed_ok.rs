//@check-pass
// spec: Requirement (Step), Definition (Effect sets): a step runs counted
// loops, and performs Sigma_C effects (send, now). math::f32::exp is trame's own function, trusted at every level.
// This is the old fixture step_ok, against the real surface.
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
    let mut buffer = [0u32; 4];
    let mut sum = [0u32; 4];
    for i in 0..4usize {
        sum[i] = i as u32;
        buffer[i] = sum[i];
    }
    let _ = trame::send(trame::LEADER, Msg { n: q.n });
    let _ = trame::now();
    q.n = trame::math::f32::exp(1.0) as u32 + sum[3] + buffer[3];
    trame::Status::Done
}

fn main() {}

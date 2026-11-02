//@check-pass
// backend.rs M2, M3: plain data is accepted: numbers, Vec<u64>, String, Box<[f32]>, Option, tuples, arrays.

#[trame::state(steps(step))]
struct Hold {
    n: u64,
    v: Vec<u64>,
    s: String,
    w: Box<[f32]>,
    o: Option<u32>,
    t: (u8, i16),
    a: [f64; 4],
    d: std::time::Duration,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

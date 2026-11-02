//@check-pass
// backend.rs M2, M3: a plain pool type is accepted.
struct Table {
    weights: Vec<f32>,
    names: Vec<String>,
}

#[trame::state(steps(step), pool = Table)]
struct Hold {
    n: u64,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

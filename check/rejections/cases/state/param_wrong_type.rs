// spec: a step parameter has the type of the field it names. `n` is a u32 field and the step takes it as a u64.
#[trame::state(steps(step))]
struct S {
    n: u32,
}

#[trame::step(S)] //~ ERROR: mismatched types
fn step(n: &mut u64) -> trame::Status {
    *n += 1;
    trame::Status::Done
}

fn main() {}

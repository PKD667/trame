// spec: CONTRACT.md, M2. The pool type need not be a struct: `pool = Vec<fn(u32) -> u32>` is read as a type, and the
// refusal has no field of the program to point at, so it carries no source line.
//@error-in-other-file: [state-memory] pool: std::vec::Vec<fn(u32) -> u32> holds a function pointer
#[trame::state(steps(step), pool = Vec<fn(u32) -> u32>)]
struct Hold {
    n: u64,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

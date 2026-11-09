// spec: CONTRACT.md, M2. The pool type need not be a struct: `pool = Vec<fn(u32) -> u32>` is a type, and
// State::Pool's bound refuses it at the attribute.
#[trame::state(steps(step), pool = Vec<fn(u32) -> u32>)] //~ ERROR: cannot be held by a trame state, pool or error
struct Hold {
    n: u64,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

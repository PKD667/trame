// spec: CONTRACT.md, M2 (no process meaning): a state holds no code address, since a backend may run the state's steps where the function does
// not live at that address. A function pointer is refused at the field.

#[trame::state(steps(step))]
struct Hold {
    cb: fn(u32) -> u32, //~ ERROR: cannot be held by a trame state, pool or error
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

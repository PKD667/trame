// spec: CONTRACT.md, M2 (no process meaning): a variant of a user enum is read like a field.
#[derive(trame::Plain)]
enum Mode {
    Idle,
    Call(fn()), //~ ERROR: cannot be held by a trame state, pool or error
}

#[trame::state(steps(step))]
struct Hold {
    mode: Mode,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

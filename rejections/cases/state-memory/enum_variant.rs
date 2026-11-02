// spec: CONTRACT.md, M2 (no process meaning): a variant of a user enum is read like a field.
enum Mode {
    Idle,
    Call(fn()),
}

#[trame::state(steps(step))]
struct Hold {
    mode: Mode, //~ ERROR: [state-memory] Hold.mode.0: fn() holds a function pointer (a code address) (CONTRACT.md, M2)
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

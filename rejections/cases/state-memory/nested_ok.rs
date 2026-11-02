//@check-pass
// CONTRACT.md, M2, M3: states and plain structs nest; a recursive type ends the walk, and a PhantomData holds nothing.
use trame::WorkerId;

struct Node {
    v: u32,
    next: Option<Box<Node>>,
}

enum Mode {
    Idle,
    Busy(u32, WorkerId),
}

struct Inner {
    mode: Mode,
    list: Option<Box<Node>>,
    pad: std::marker::PhantomData<fn()>,
}

#[trame::state(steps(step))]
struct Hold {
    inner: Inner,
    more: Vec<Inner>,
}

#[trame::step(Hold)]
fn step() -> trame::Status {
    trame::Status::Done
}

fn main() {}

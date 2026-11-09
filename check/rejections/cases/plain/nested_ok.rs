//@check-pass
// CONTRACT.md, M2, M3: states and plain structs nest; user types derive Plain, a recursive type checks, and a PhantomData holds nothing.
use trame::WorkerId;

#[derive(trame::Plain)]
struct Node {
    v: u32,
    next: Option<Box<Node>>,
}

#[derive(trame::Plain)]
enum Mode {
    Idle,
    Busy(u32, WorkerId),
}

#[derive(trame::Plain)]
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

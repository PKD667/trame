// spec: Requirement (All levels): drop glue counts as a call. Box<u32> has no Drop impl of its own: its glue is library
// code, and a step that makes and drops one allocates and frees (CONTRACT.md, M4, D5): `Box::new` and the drop are
// refused as allocation. Its raw pointer derefs are the glue's own, not [unsafe].
#[trame::message]
struct M {
    n: u32,
}

#[derive(trame::Plain)]
struct Q {
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    let b = Box::new(q.n); //~ ERROR: [allocation] step
    q.n = *b + 1;
    trame::Status::Done
} //~ ERROR: [allocation] step

fn main() {}

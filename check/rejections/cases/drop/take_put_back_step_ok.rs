//@check-pass
// spec: Requirement (All levels): drop glue counts as a call. A step that takes a Plain value owning buffers out of its
// state, lends it to a method that also takes &mut self, and puts it back frees nothing; `trame::lend` is the taking and
// the putting back, which asserts the slot is empty so no old value is dropped (nerve, point (a)). The pair of take_assign_step.rs.
#[derive(trame::Plain)]
struct W {
    v: Vec<u32>,
}

#[derive(trame::Plain)]
struct Delivery {
    w: Option<W>,
    n: u32,
}

#[trame::state(steps(step))]
struct S {
    d: Delivery,
}

#[trame::step(S)]
fn step(d: &mut Delivery) -> trame::Status {
    trame::lend(d, |d| &mut d.w, |d, w| d.n += w.v.len() as u32);
    trame::Status::Progress
}

fn main() {}

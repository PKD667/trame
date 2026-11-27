// spec: Requirement (All levels): drop glue counts as a call, except a core or alloc drop on the unwind path of a panic
// `guard` owns the Vec and gives it back; the only path that drops it is the one out of `assert!`, so
// `guard` draws no diagnostic (before the exemption it drew [allocation] step at its `assert!`). The step's own `*v = w`
// drops the old buffer on its normal path, which is still refused: exactly one diagnostic, on that line. The pair of
// unwind_vec_normal_step.rs, where the normal path of the callee drops.
#[trame::state(steps(step))]
struct S {
    v: Vec<u32>,
}

fn guard(v: Vec<u32>) -> Vec<u32> {
    assert!(v.len() < 1000);
    v
}

#[trame::step(S)]
fn step(v: &mut Vec<u32>) -> trame::Status {
    let w = guard(core::mem::take(v));
    *v = w; //~ ERROR: [allocation] step
    trame::Status::Done
}

fn main() {}

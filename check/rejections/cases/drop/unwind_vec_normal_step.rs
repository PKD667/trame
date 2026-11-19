// spec: Requirement (All levels): drop glue counts as a call. The pair of unwind_vec_step_ok.rs: `eat` also drops the Vec
// on its normal path, which frees a buffer, and a step never frees. The exemption is for the unwind path only.
#[trame::state(steps(step))]
struct S {
    v: Vec<u32>,
}

fn eat(v: Vec<u32>) -> u32 {
    assert!(v.len() < 1000);
    v.len() as u32
} //~ ERROR: [allocation] step

#[trame::step(S)]
fn step(v: &mut Vec<u32>) -> trame::Status {
    let _n = eat(core::mem::take(v));
    trame::Status::Done
}

fn main() {}

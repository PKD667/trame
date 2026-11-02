// spec: Requirement (All levels): no unsafe. A step is a plain fn: the step macro refuses `unsafe fn`, so no unsafe
// signature reaches the driver. The driver's rule for the body is covered by unsafe/block_step.rs.
#[trame::state(steps(step))]
struct S {
    n: u32,
}

#[trame::step(S)]
unsafe fn step(n: &mut u32) -> trame::Status { //~ ERROR: a step is a plain fn: not async, not unsafe
    *n += 1;
    trame::Status::Done
}

fn main() {}

// spec: backend.rs M4, D5. Vec::extend is allowed by the trait path of the iterator lists (core::iter::*) and grows the
// buffer. The driver refuses a call into alloc that only a trait path allows: A_C.txt names the alloc calls a step
// may make by the alloc type's own key, and `alloc::vec::Vec::extend` is not one.
#[trame::state(steps(step))]
struct S {
    v: Vec<u32>,
}

#[trame::step(S)]
fn step(v: &mut Vec<u32>) -> trame::Status {
    v.extend([1u32, 2]); //~ ERROR: [allocation] step
    trame::Status::Done
}

fn main() {}

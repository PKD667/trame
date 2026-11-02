// spec: backend.rs M4, D5: a step neither allocates nor frees. A local Vec is made and dropped inside the step: Vec::new
// is a call into alloc, and the drop at the end is a free, which the drop glue's `Vec::drop` shows.
#[trame::state(steps(step))]
struct S {
    n: u32,
}

#[trame::step(S)]
fn step(n: &mut u32) -> trame::Status {
    let v: Vec<u32> = Vec::new(); //~ ERROR: [allocation] step
    *n = v.len() as u32;
    trame::Status::Done
} //~ ERROR: [allocation] step

fn main() {}

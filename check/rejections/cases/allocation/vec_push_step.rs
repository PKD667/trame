// spec: CONTRACT.md, M4, D5: a step neither allocates nor frees. The pair of vec_index_assign_step_ok.rs: the same field,
// grown instead of rewritten. Vec::push may grow the buffer, and only a builder or an edit allocates in the domain.
#[trame::state(steps(step))]
struct S {
    v: Vec<u32>,
}

#[trame::step(S)]
fn step(v: &mut Vec<u32>) -> trame::Status {
    v.push(1); //~ ERROR: [allocation] step
    trame::Status::Done
}

fn main() {}

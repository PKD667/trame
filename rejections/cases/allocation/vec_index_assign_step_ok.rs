//@check-pass
// spec: backend.rs M4: a step rewrites the buffers its state holds, in place. Index assignment, len, is_empty,
// iter_mut and a counted loop over the indices neither grow nor free the Vec. The pair of vec_push_step.rs.
#[trame::state(steps(step))]
struct S {
    v: Vec<u64>,
}

#[trame::step(S)]
fn step(v: &mut Vec<u64>) -> trame::Status {
    if v.is_empty() {
        return trame::Status::Done;
    }
    v[0] = 1;
    for i in 1..v.len() {
        v[i] = v[i - 1] + 1;
    }
    for x in v.iter_mut() {
        *x += 1;
    }
    trame::Status::Done
}

fn main() {}

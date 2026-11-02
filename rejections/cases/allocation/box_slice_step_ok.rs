//@check-pass
// spec: CONTRACT.md, M4: a step rewrites a Box<[f32]> field in place: an index assignment through the box neither
// allocates nor frees. The box is allocated by the builder.
#[trame::state(steps(step))]
struct S {
    w: Box<[f32]>,
}

#[trame::step(S)]
fn step(w: &mut Box<[f32]>) -> trame::Status {
    for i in 0..w.len() {
        w[i] = w[i] * 0.5;
    }
    trame::Status::Done
}

fn main() {}

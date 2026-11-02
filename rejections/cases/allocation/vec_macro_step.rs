// spec: CONTRACT.md, M4, D5: vec![] allocates (a box expression), and the assignment frees the old buffer.
// The diagnostics are at the macro's call site.
#[trame::state(steps(step))]
struct S {
    out: Vec<u32>,
}

#[trame::step(S)]
fn step(out: &mut Vec<u32>) -> trame::Status {
    *out = vec![1]; //~ ERROR: [allocation] step
    //~^ ERROR: [allocation] step
    //~| ERROR: [allocation] step
    trame::Status::Done
}

fn main() {}

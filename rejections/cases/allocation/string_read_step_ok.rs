//@check-pass
// spec: backend.rs M4: a step reads a String field without growing it: len, is_empty, as_str, a comparison.
#[trame::state(steps(step))]
struct S {
    s: String,
    n: usize,
}

#[trame::step(S)]
fn step(s: &String, n: &mut usize) -> trame::Status {
    if s.is_empty() || s.as_str() == "stop" {
        return trame::Status::Done;
    }
    *n = s.len();
    trame::Status::Done
}

fn main() {}

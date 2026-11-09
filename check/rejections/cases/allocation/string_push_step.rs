// spec: CONTRACT.md, M4, D5: a String is a Vec<u8>: push_str grows it, so it is refused as Vec::push is; len and as_str read it.
// The pair of string_read_step_ok.rs.
#[trame::state(steps(step))]
struct S {
    s: String,
}

#[trame::step(S)]
fn step(s: &mut String) -> trame::Status {
    s.push_str("x"); //~ ERROR: [allocation] step
    trame::Status::Done
}

fn main() {}

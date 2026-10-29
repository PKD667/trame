// spec: Requirement (Messages) does not state this. The macro refuses a generic message struct: "a message type cannot be
// generic" (macros/lib.rs:31-32).
#[trame::message]
struct Wrapped<T> { //~ ERROR: a message type cannot be generic
    v: T,
}

fn main() {}

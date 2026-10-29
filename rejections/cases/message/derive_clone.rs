// spec: Requirement (Messages) states that a message is plain data; it does not state the derive rule. The macro derives Clone
// and Copy itself and refuses a user derive of them (macros/lib.rs:20, 88-91), so the set is fixed.
#[trame::message]
#[derive(Clone)] //~ ERROR: #[trame::message] derives Clone and Copy itself; remove this
struct Twice {
    a: u32,
}

fn main() {}

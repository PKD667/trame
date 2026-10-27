//! expect: error derives Clone and Copy itself
// spec: Section 3 (Compile time), Requirement (Message): the macro derives Clone and Copy itself; a user derive
// of them is refused, so the set is fixed.
#[trame::message]
#[derive(Clone)]
struct Twice {
    a: u32,
}

fn main() {}

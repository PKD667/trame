//! expect: error a message type cannot be generic
// spec: Section 3 (Compile time), Requirement (Message): a message has one concrete layout, so it is not generic.
#[trame::message]
struct Wrapped<T> {
    v: T,
}

fn main() {}

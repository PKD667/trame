//! expect: error a message type may only be `repr(C)`
// spec: Section 3 (Compile time), Requirement (Message): the layout is repr(C). repr(packed) changes it, and
// is refused.
#[trame::message]
#[repr(packed)]
struct Squeezed {
    a: u8,
    b: u32,
}

fn main() {}

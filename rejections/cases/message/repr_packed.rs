// spec: Section 3 (Compile time), Requirement (Message): the layout is repr(C). repr(packed) changes it, and
// is refused.
#[trame::message]
#[repr(packed)] //~ ERROR: a message type may only be `repr(C)`
struct Squeezed {
    a: u8,
    b: u32,
}

fn main() {}

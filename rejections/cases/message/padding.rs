//! expect: error padding
// spec: Section 3 (Compile time), Requirement (Message): a message has no padding bytes, because padding is
// uninitialized and would cross the wire. The bytemuck NoUninit check (macros/lib.rs) refuses it.
#[trame::message]
struct Padded {
    a: u8,
    b: u32,
}

fn main() {}

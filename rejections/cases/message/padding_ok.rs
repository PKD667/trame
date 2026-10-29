//@check-pass
// spec: Section 3 (Compile time), Requirement (Message). This is the pair of padding.rs: the same two fields,
// with the u8 widened to a u32, leave no padding. Accepted.
#[trame::message]
struct Packed {
    a: u32,
    b: u32,
}

fn main() {
    let _ = Packed { a: 1, b: 2 };
}

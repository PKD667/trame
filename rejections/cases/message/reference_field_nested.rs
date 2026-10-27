//! expect: error a message field may not be a reference
// spec: Section 3 (Compile time), Requirement (Message). The token scan sees through an array: a reference
// inside `[&u32; 2]` is still a reference.
#[trame::message]
struct Nested {
    refs: [&'static u32; 2],
}

fn main() {}

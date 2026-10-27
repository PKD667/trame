//! expect: error a message field may not be a raw pointer
// spec: Section 3 (Compile time), Requirement (Message): a raw pointer is an address in one machine's memory.
#[trame::message]
struct Addressed {
    p: *const u32,
}

fn main() {}

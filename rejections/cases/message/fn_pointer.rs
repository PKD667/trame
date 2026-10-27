//! expect: error a message field may not be a function pointer
// spec: Section 3 (Compile time), Requirement (Message): a function pointer is an address in one binary.
#[trame::message]
struct Callable {
    f: fn(u32) -> u32,
}

fn main() {}

// spec: Section 3 (Compile time), Requirement (Messages): a message field is plain data, not a reference: a
// reference is meaningless on another machine. (No lifetime parameter: a generic is refused first, by its
// own rule, so this case uses a 'static reference to reach the reference rule.)
#[trame::message]
struct Borrowed {
    s: &'static str, //~ ERROR: a message field may not be a reference
}

fn main() {}

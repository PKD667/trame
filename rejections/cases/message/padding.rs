// spec: Requirement (Messages): a message is plain data with no padding, because padding is uninitialized and would cross
// the wire. The macro derives bytemuck's Pod, which checks for padding (macros/lib.rs:16-17, 44). Its error is
// "derive(Pod) was applied to a type with padding".
#[trame::message] //~ ERROR: evaluation panicked: derive(Pod) was applied to a type with padding
struct Padded {
    a: u8,
    b: u32,
}

fn main() {}

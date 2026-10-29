//@check-pass
// spec: Section 3 (Compile time), Requirement (Message): a plain struct of u32, f32 and arrays is a message.
// This is the boundary the refusals above sit next to.
#[trame::message]
struct Reading {
    id: u32,
    level: f32,
    samples: [u32; 4],
}

fn main() {
    let _ = Reading { id: 1, level: 0.5, samples: [0; 4] };
}

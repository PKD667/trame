//@check-pass
// spec: Section 3 (Compile time), Requirement (Messages): a plain-data type is a message, arrays included. A field
// of [Spike; 4] (an array of a message struct) makes the outer struct a message, and the array is a message type.
#[trame::message]
struct Spike {
    neuron: u32,
    t: u32,
}

#[trame::message]
struct Burst {
    id: u32,
    spikes: [Spike; 4],
}

fn need<M: trame::Message>() {}

fn main() {
    need::<Burst>();
    need::<[Spike; 4]>();
    let _ = Burst { id: 1, spikes: [Spike { neuron: 0, t: 0 }; 4] };
}

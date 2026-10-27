//! expect: accepted
// spec: Section 5.3 (Processes and cells): `receives` takes any message type, an array included, since an array of
// a message struct is one (Section 3, Messages).
#[trame::message]
struct Spike {
    neuron: u32,
}

struct Pending(Vec<u32>);

struct P;

#[trame::process(receives = [Spike; 4])]
impl P {
    fn step(&mut self, p: &mut Pending) -> trame::Status {
        p.0.push(1);
        trame::Status::Done
    }
}

fn main() {
    let mut p = Pending(Vec::new());
    let _ = trame::concurrent!(P; p);
}

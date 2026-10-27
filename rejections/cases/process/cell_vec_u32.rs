//! expect: error a primitive or core/alloc type, which cannot be a cell
// spec: Section 5.3 (Processes and cells): a Vec is a core/alloc type and cannot be a cell. Wrap it in a type
// of your own (the macro's own suggestion).
#[trame::message]
struct Job {
    id: u32,
}

struct P;

#[trame::process(receives = Job)]
impl P {
    fn step(&mut self, v: &mut Vec<u32>) -> trame::Status {
        v.push(1);
        trame::Status::Done
    }
}

fn main() {}

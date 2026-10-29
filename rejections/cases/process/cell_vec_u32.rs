// spec: Section 5.3 (Scheduling), Definition (Processes): each cell has a type of its own. A Vec is a core/alloc type, so it
// cannot be a cell (lib.rs:40; macros/process.rs:147). Wrap it in a type of your own (the macro's own suggestion).
#[trame::message]
struct Job {
    id: u32,
}

struct P;

#[trame::process(receives = Job)]
impl P {
    fn step(&mut self, v: &mut Vec<u32>) -> trame::Status { //~ ERROR: step `step`: parameter `v` has the type `Vec < u32 >`, a primitive or core/alloc type, which cannot be a cell. Fix: wrap it in a type of your own, e.g. struct Pending(Vec<Job>)
        v.push(1);
        trame::Status::Done
    }
}

fn main() {}

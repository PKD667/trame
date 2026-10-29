// spec: Section 5.3 (Scheduling), Definition (Processes): each cell has a type of its own. A primitive or core/alloc type is
// refused, since it is not a type of the program (lib.rs:40; macros/process.rs:147). The process macro refuses a u32 cell.
#[trame::message]
struct Job {
    id: u32,
}

struct P;

#[trame::process(receives = Job)]
impl P {
    fn step(&mut self, n: &mut u32) -> trame::Status { //~ ERROR: step `step`: parameter `n` has the type `u32`, a primitive or core/alloc type, which cannot be a cell. Fix: wrap it in a type of your own, e.g. struct Pending(Vec<Job>)
        *n += 1;
        trame::Status::Done
    }
}

fn main() {}

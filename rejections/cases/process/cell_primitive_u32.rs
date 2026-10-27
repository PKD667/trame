//! expect: error a primitive or core/alloc type, which cannot be a cell
// spec: Section 5.3 (Processes and cells): a cell is a type of its own, not a primitive or a core/alloc type,
// so that two cells of one type cannot be confused. The process macro refuses a u32 cell.
#[trame::message]
struct Job {
    id: u32,
}

struct P;

#[trame::process(receives = Job)]
impl P {
    fn step(&mut self, n: &mut u32) -> trame::Status {
        *n += 1;
        trame::Status::Done
    }
}

fn main() {}

//@check-pass
// spec: Section 5.3 (Scheduling), Definition (Processes): cells are matched to steps by type, and each cell has a type of its
// own. This is the pair of cell_primitive_u32.rs and cell_vec_u32.rs: the cell is a newtype over the Vec, which is its own
// type, and is accepted.
#[trame::message]
struct Job {
    id: u32,
}

struct Pending(Vec<u32>);

struct P;

#[trame::process(receives = Job)]
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

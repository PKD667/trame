//! runner: plain cargo
// spec: Requirement (Checked builds): without --cfg trame_checked, concurrent! expands to compile_error!. The error says how
// to build. This case runs under plain cargo (see the runner line), so the flag is not set.
#[trame::message]
struct Job {
    id: u32,
}

struct Queue {
    n: u32,
}

struct P;

#[trame::process(receives = Job)]
impl P {
    fn step(&mut self, q: &mut Queue) -> trame::Status {
        q.n += 1;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q); //~ ERROR: build with cargo trame
}

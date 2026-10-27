//! expect: error build with cargo trame
//! runner: plain cargo
// spec: Section 5.3 (Processes and cells): concurrent! needs the driver's checks. Without --cfg trame_checked the
// macro is a compile error that says how to build. Run under plain cargo (runner = plain cargo).
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
    let _ = trame::concurrent!(P; q);
}

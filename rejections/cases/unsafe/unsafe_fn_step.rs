//! expect: error call to unsafe function
// spec: Requirement (All levels): no unsafe. A step declared `unsafe fn` is unsafe code. The concurrent! expansion
// calls the step from a generated impl, outside any unsafe block, so rustc refuses it (E0133) before the driver
// runs. The driver's own rule for an unsafe signature is therefore not reached through the public macro
// (rejections/README.md, finding E1).
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
    unsafe fn step(&mut self, q: &mut Queue) -> trame::Status {
        q.n += 1;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

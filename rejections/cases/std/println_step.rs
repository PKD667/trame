//! expect: refused [std] step, [unsafe] step
// spec: Requirement (All levels): no std. A step prints, which calls std. The unsafe rule also fires on core's
// `fmt::Arguments::new` (rejections/README.md, finding D1).
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
        std::println!("n = {}", q.n);
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

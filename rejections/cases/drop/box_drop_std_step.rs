// spec: Requirement (All levels): no std, and drop glue counts as a call. The step drops a Box<Loud>; Loud::drop calls
// std, reached through the glue of Box (an Own edge), so the step is refused by the std rule. Steps may allocate, so
// the Box itself is not refused.
struct Loud(u32);

impl Drop for Loud {
    fn drop(&mut self) {
        std::thread::yield_now(); //~ ERROR: [std] step
    }
}

#[trame::message]
struct M {
    n: u32,
}

struct Q {
    b: Box<Loud>,
}

struct P;

#[trame::process(receives = M)]
impl P {
    fn step(&mut self, q: &mut Q) -> trame::Status {
        q.b = Box::new(Loud(1));
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { b: Box::new(Loud(0)) };
    let _ = trame::concurrent!(P; q);
}

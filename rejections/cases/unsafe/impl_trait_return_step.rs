// spec: Requirement (All levels): no unsafe. The returned impl Fn hides a closure with unsafe code; the step calls it.
fn make(k: u32) -> impl Fn(u32) -> u32 {
    move |x| { let p = &x as *const u32; unsafe { *p + k } }
    //~^ ERROR: [unsafe] step
    //~| ERROR: [unsafe] step
}

#[trame::message]
struct M {
    n: u32,
}

struct Q {
    n: u32,
}

struct P;

#[trame::process(receives = M)]
impl P {
    fn step(&mut self, q: &mut Q) -> trame::Status {
        q.n = make(1)(q.n);
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { n: 1 };
    let _ = trame::concurrent!(P; q);
}

// spec: Requirement (All levels): no FFI. A step calls a C function declared in an extern block. Rust makes the call an
// unsafe block, so [unsafe] names it too: both rules are true, and both are reported (as ffi/extern_item.rs).
unsafe extern "C" {
    fn abs(x: i32) -> i32;
}

#[trame::message]
struct M {
    n: u32,
}

struct Q {
    n: i32,
}

struct P;

#[trame::process(receives = M)]
impl P {
    fn step(&mut self, q: &mut Q) -> trame::Status {
        q.n = unsafe { abs(q.n) };
        //~^ ERROR: [unsafe] step
        //~| ERROR: [ffi] step
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { n: -1 };
    let _ = trame::concurrent!(P; q);
}

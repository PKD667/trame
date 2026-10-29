// spec: Requirement (All levels): drop glue counts as a call, and a program's own Drop impl is judged in full. Dropping
// Box<X> calls the glue of Box, which reaches X::drop (the Own edge). X::drop holds unsafe code, so the step is refused.
struct X(u32);

impl Drop for X {
    fn drop(&mut self) {
        let p = &self.0 as *const u32;
        let _ = unsafe { *p }; //~ ERROR: [unsafe] step
    }
}

#[trame::message]
struct M {
    n: u32,
}

struct Q {
    b: Box<X>,
}

struct P;

#[trame::process(receives = M)]
impl P {
    fn step(&mut self, q: &mut Q) -> trame::Status {
        q.b = Box::new(X(1));
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { b: Box::new(X(0)) };
    let _ = trame::concurrent!(P; q);
}

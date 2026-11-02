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

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    q.b = Box::new(X(1)); //~ ERROR: [allocation] step
    //~^ ERROR: [allocation] step
    trame::Status::Done
}

fn main() {}

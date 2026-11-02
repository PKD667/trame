// spec: Requirement (All levels): no unsafe, and drop glue counts as a call. Dropping a Vec<X> runs X::drop
// (core's drop_in_place loops over the elements); X::drop holds unsafe code. The step drops the Vec by overwriting it.
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
    v: Vec<X>,
}

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    q.v = Vec::new(); //~ ERROR: [allocation] step
    //~^ ERROR: [allocation] step
    trame::Status::Done
}

fn main() {}

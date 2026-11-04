// spec: Requirement (All levels): no unsafe, and a user Clone impl is user code. The step clones an array of X;
// `[X; N]::clone` is core code that calls X::clone, whose body holds an unsafe block.
#[derive(trame::Plain)]
struct X(u32);

impl Clone for X {
    fn clone(&self) -> X {
        let p = &self.0 as *const u32;
        X(unsafe { *p })
        //~^ ERROR: [unsafe] step
        //~| ERROR: [unsafe] step
    }
}

#[trame::message]
struct M {
    n: u32,
}

#[derive(trame::Plain)]
struct Q {
    a: [X; 2],
}

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    let b = q.a.clone();
    q.a = b;
    trame::Status::Done
}

fn main() {}

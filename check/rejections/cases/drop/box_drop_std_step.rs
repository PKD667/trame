// spec: Requirement (All levels): no std, and drop glue counts as a call. The step drops a Box<Loud>; Loud::drop calls
// std, reached through the glue of Box (an Own edge), so the step is refused by the std rule. The Box::new and the drop are
// refused too: a step neither allocates nor frees (CONTRACT.md, M4).
#[derive(trame::Plain)]
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

#[derive(trame::Plain)]
struct Q {
    b: Box<Loud>,
}

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    q.b = Box::new(Loud(1)); //~ ERROR: [allocation] step
    //~^ ERROR: [allocation] step
    trame::Status::Done
}

fn main() {}

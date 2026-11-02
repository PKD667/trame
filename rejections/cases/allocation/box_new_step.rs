// spec: backend.rs M4, D5: Box::new allocates, and the assignment frees the old box. The pair of box_slice_step_ok.rs.
#[trame::state(steps(step))]
struct S {
    b: Box<u32>,
}

#[trame::step(S)]
fn step(b: &mut Box<u32>) -> trame::Status {
    **b += 1;
    *b = Box::new(**b);
    //~^ ERROR: [allocation] step
    //~| ERROR: [allocation] step
    trame::Status::Done
}

fn main() {}

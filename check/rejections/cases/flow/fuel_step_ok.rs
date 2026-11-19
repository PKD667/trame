//@check-pass
// spec: Section 7 (fuel): a while loop with its bound in the call. `fuel::<N>` runs the body at most N times; the step may
// treat running out as an abort or as a reason to run again. A `while` written by hand is still refused (counted-loop/).
use core::ops::ControlFlow;

#[derive(trame::Plain)]
struct Counter {
    left: u32,
    steps: u32,
}

#[trame::state(steps(step))]
struct S {
    c: Counter,
}

#[trame::step(S)]
fn step(c: &mut Counter) -> trame::Status {
    let r = trame::fuel::<1000>(|| {
        if c.left == 0 {
            return ControlFlow::Break(());
        }
        c.left -= 1;
        c.steps += 1;
        ControlFlow::Continue(())
    });
    match r {
        Ok(_) => trame::Status::Done,
        Err(trame::Spent) => trame::Status::Progress,
    }
}

fn main() {}

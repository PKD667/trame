//! err_done: one worker with two steps. `finishes` (declared first) returns Done at its first step.
//! `fails` (declared second) returns an error at its first step. Both have ended, and the worker's
//! outcome is `Failed` with the error of `fails`. A worker that answered `Returned` here would have
//! ignored the error, because a step that returned Done is not a step that is still running. The leader
//! prints the error code and the outcomes. (Before the worker model a worker body could catch the error
//! and return normally, so this printed `Returned`.)

use trame::{Outcome, Status, Topology};

#[derive(Debug, PartialEq, Eq)]
struct Boom(u32);

#[trame::state(steps(finishes, fails), error = Boom)]
struct Pair {}

#[trame::step(Pair)]
fn finishes() -> Result<Status, Boom> {
    Ok(Status::Done)
}

#[trame::step(Pair)]
fn fails() -> Result<Status, Boom> {
    Err(Boom(7))
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(Pair {});
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let outcomes: Vec<Outcome> = run.join();
    let boom = w.error().expect("a failed worker keeps its error");
    println!("worker reports error {}", boom.0);
    println!("outcomes: {outcomes:?}");
}

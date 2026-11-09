//! err: one worker with two steps. `fails` (declared first) returns an error at its first step. `idle`
//! (declared second) waits with a bare `Status::wait()` and records that it ran. A step's error ends the
//! worker: the outcome is `Failed`, not `Returned`, `Worker::error` answers the error, and no step starts
//! after the failure, so `idle` never ran. The leader prints the error code and the outcomes. (Before the
//! worker model a worker body could catch the error and return normally, so this printed `Returned`.)

use trame::{Outcome, Status, Topology};

#[derive(Debug, PartialEq, Eq, trame::Plain)]
struct Boom(u32);

/// The state: whether `idle` ever ran.
#[trame::state(steps(fails, idle), error = Boom)]
struct Pair {
    idled: bool,
}

#[trame::step(Pair)]
fn fails() -> Result<Status, Boom> {
    Err(Boom(7))
}

#[trame::step(Pair)]
fn idle(idled: &mut bool) -> Result<Status, Boom> {
    *idled = true;
    Ok(Status::wait())
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker();
    t.set(&w, move || Pair { idled: false });
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
    assert!(!w.take().idled, "a step started after another failed");
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

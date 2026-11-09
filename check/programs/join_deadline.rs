//! join_deadline: the leader joins while a worker waits on a deadline of its own clock (20 ms after its
//! first step). The join must hold until the worker's step wakes, returns Done, and the worker finishes.
//! The leader prints one line before the join and the outcomes after it; the join's own timing is not
//! printed.

use trame::{Outcome, Reading, Status, Topology};

const WAIT_NANOS: u64 = 20_000_000;

/// The state: the deadline; `None` until the first step fixes it.
#[trame::state(steps(sleeper))]
struct Sleeper {
    deadline: Option<Reading>,
}

/// Waits for its deadline, then returns.
#[trame::step(Sleeper)]
fn sleeper(deadline: &mut Option<Reading>) -> Status {
    let now = trame::now();
    let deadline = *deadline.get_or_insert_with(|| now.after(core::time::Duration::from_nanos(WAIT_NANOS)));
    if deadline.since(now) > core::time::Duration::ZERO {
        return Status::wait().until(deadline);
    }
    Status::Done
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker();
    t.set(&w, || Sleeper { deadline: None });
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    println!("joining");
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

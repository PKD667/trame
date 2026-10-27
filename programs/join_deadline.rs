//! join_deadline: the leader joins while a worker waits on a deadline of its own clock (20 ms after its
//! first step). The join must hold until the worker's process wakes, returns, and the worker finishes.
//! The leader prints one line before the join and the outcomes after it; the join's own timing is not
//! printed.

use trame::{Context, Outcome, Reading, Status, Topology};

const WAIT_NANOS: u64 = 20_000_000;

/// Waits for its deadline, then returns; `None` until the first step fixes it.
struct Sleeper {
    deadline: Option<Reading>,
}

#[trame::process]
impl Sleeper {
    fn step(&mut self) -> Status {
        let now = trame::now();
        let deadline = *self.deadline.get_or_insert_with(|| now.after(core::time::Duration::from_nanos(WAIT_NANOS)));
        if deadline.since(now) > core::time::Duration::ZERO {
            return Status::wait().until(deadline);
        }
        Status::Done
    }
}

fn worker(_cx: &Context) {
    let r = trame::concurrent!(Sleeper { deadline: None };);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    t.worker(worker);
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

//! wait_until: one worker, one step. It waits until its own clock is 20 ms past its first step
//! (`Status::wait().until`). It logs only once the deadline has passed, so an early wake changes nothing. The
//! leader prints that it woke; no times are printed.

use trame::{LEADER, Outcome, Reading, Status, Topology};

const WAIT_NANOS: u64 = 20_000_000;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Woke {
    id: u32,
}

/// The state: the deadline; `None` until the first step fixes it.
#[trame::state(steps(sleeper))]
struct Sleeper {
    deadline: Option<Reading>,
}

/// Waits for its deadline, then reports.
#[trame::step(Sleeper)]
fn sleeper(deadline: &mut Option<Reading>) -> Status {
    let now = trame::now();
    let deadline = *deadline.get_or_insert_with(|| now.after(core::time::Duration::from_nanos(WAIT_NANOS)));
    if deadline.since(now) > core::time::Duration::ZERO {
        return Status::wait().until(deadline);
    }
    assert!(trame::send(LEADER, Woke { id: 1 }).is_ok(), "a report did not fit");
    Status::Done
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(Sleeper { deadline: None }).id();
    t.route::<Woke>(w, LEADER, 2);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    loop {
        if let Some(w) = run.recv::<Woke>() {
            println!("woke at the deadline (report {})", w.id);
            break;
        }
        run.wait();
    }
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

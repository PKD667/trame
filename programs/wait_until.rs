//! wait_until: one worker, one process. It waits until its own clock is 20 ms past its first step
//! (`Status::wait().until`). It logs only once the deadline has passed, so an early wake changes nothing. The
//! leader prints that it woke; no times are printed.

use trame::{Context, Outcome, Reading, Status, Topology, LEADER};

const WAIT_NANOS: u64 = 20_000_000;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Woke {
    id: u32,
}

/// Waits for its deadline; `None` until the first step fixes it.
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
        assert!(trame::send(LEADER, Woke { id: 1 }).is_ok(), "a report did not fit");
        Status::Done
    }
}

fn worker(_cx: &Context) {
    let r = trame::concurrent!(Sleeper { deadline: None };);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(worker);
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

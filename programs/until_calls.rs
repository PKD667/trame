//! until_calls: one worker, one process. It waits until its clock is 20 ms past its first step. A process
//! is stepped only when its wait condition holds (spec.md 5.3, Enabled): it is called once to fix the
//! deadline, and once when the deadline has passed. It reports how many times it was called. An early wake
//! (a wait condition that holds before the deadline) shows as a larger count.

use trame::{Context, LEADER, Outcome, Reading, Status, Topology};

const WAIT_NANOS: u64 = 20_000_000;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Woke {
    calls: u32,
}

struct Sleeper {
    deadline: Option<Reading>,
    calls: u32,
}

#[trame::process]
impl Sleeper {
    fn step(&mut self) -> Status {
        self.calls += 1;
        let now = trame::now();
        let deadline = *self.deadline.get_or_insert_with(|| now.after(core::time::Duration::from_nanos(WAIT_NANOS)));
        if deadline.since(now) > core::time::Duration::ZERO {
            return Status::wait().until(deadline);
        }
        assert!(trame::send(LEADER, Woke { calls: self.calls }).is_ok(), "a report did not fit");
        Status::Done
    }
}

fn worker(_cx: &Context) {
    let r = trame::concurrent!(Sleeper { deadline: None, calls: 0 };);
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
    let woke = loop {
        if let Some(w) = run.recv::<Woke>() {
            break w;
        }
        run.wait();
    };
    println!("woke at the deadline after {} calls", woke.calls);
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

//! until_calls: one worker, one step. It waits until its clock is 20 ms past its first step. A step is
//! started only when its wait condition holds (sched.rs, Enabled): it is called once to fix the
//! deadline, and once when the deadline has passed. It reports how many times it was called. An early wake
//! (a wait condition that holds before the deadline) shows as a larger count.

use trame::{Leader, Outcome, Reading, Status, Topology};

const WAIT_NANOS: u64 = 20_000_000;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Woke {
    calls: u32,
}

#[trame::state(steps(sleeper))]
struct Sleeper {
    deadline: Option<Reading>,
    calls: u32,
}

#[trame::step(Sleeper)]
fn sleeper(deadline: &mut Option<Reading>, calls: &mut u32) -> Status {
    *calls += 1;
    let now = trame::now();
    let deadline = *deadline.get_or_insert_with(|| now.after(core::time::Duration::from_nanos(WAIT_NANOS)));
    if deadline.since(now) > core::time::Duration::ZERO {
        return Status::wait().until(deadline);
    }
    assert!(trame::send(Leader, Woke { calls: *calls }).is_ok(), "a report did not fit");
    Status::Done
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker();
    t.set(&w, move || Sleeper { deadline: None, calls: 0 });
    t.route::<Woke>(w.id(), Leader, 2);
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

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

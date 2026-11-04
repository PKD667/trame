//! Stuck runs and aborts: the run must end with the spec's results, never hang. Every test runs
//! under the 10 s guard.

extern crate std;

use super::tests::{Ping, Quick, Stuck, panic_text, within_10s};
use crate::io;
use crate::run::Outcome::{Deadlock, Returned};
use crate::status::Status;
use crate::topology::{Leader, Topology};

/// Waits for a `Ping` that never comes, and for a write that never comes.
#[crate::state(steps(waits))]
struct Waiter {}

#[crate::step(Waiter, receives = Ping)]
fn waits() -> Status {
    match io::recv::<Ping>() {
        Some(_) => Status::Done,
        None => Status::wait().frame(),
    }
}

#[crate::state(steps(boom))]
struct Boom {}

#[crate::step(Boom)]
fn boom() -> Status {
    panic!("intentional panic, for the abort test");
}

/// Sends on a route the topology does not have: the run aborts, naming the worker.
#[crate::state(steps(stray))]
struct Stray {}

#[crate::step(Stray)]
fn stray() -> Status {
    let _ = io::send(Leader, Ping(0));
    Status::Done
}

/// A worker returns while another waits for a write nothing makes (Deadlock, spec.md 5). The waiter
/// gets `Deadlock`, whichever order the two workers are declared in.
#[test]
fn returned_worker_leaves_a_waiter_deadlocked() {
    within_10s(|| {
        let mut g = Topology::new();
        let a = g.worker::<Quick>();
        g.set(&a, || Quick {});
        let b = g.worker::<Stuck>();
        g.set(&b, || Stuck { never: false });
        assert_eq!(g.launch().expect("accepted").join(), [Returned, Deadlock]);

        let mut g = Topology::new();
        let a = g.worker::<Stuck>();
        g.set(&a, || Stuck { never: false });
        let b = g.worker::<Quick>();
        g.set(&b, || Quick {});
        assert_eq!(g.launch().expect("accepted").join(), [Deadlock, Returned]);
    });
}

/// One worker with a step that is done and a step that waits for a write that the done step cannot
/// make: the worker's steps are all done or waiting, so it is `Deadlock`, not `Returned`.
#[crate::state(steps(finish, hold))]
struct Mixed {
    open: bool,
}

#[crate::step(Mixed)]
fn finish() -> Status {
    Status::Done
}

#[crate::step(Mixed)]
fn hold(open: &bool) -> Status {
    if *open { Status::Done } else { Status::wait() }
}

#[test]
fn a_worker_whose_steps_are_all_done_or_waiting_is_deadlocked() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Mixed>();
        g.set(&w, || Mixed { open: false });
        assert_eq!(g.launch().expect("accepted").join(), [Deadlock]);
    });
}

/// Join called only after the run is stuck: the leader waits until its wait panics as stuck, so the
/// stall is there before join (spec.md 5, Deadlock), and join still returns.
#[test]
fn join_after_the_run_is_stuck_returns_deadlock() {
    within_10s(|| {
        let mut g = Topology::new();
        for k in 0..3 {
            if k == 0 {
                let w = g.worker::<Quick>();
                g.set(&w, || Quick {});
            } else {
                let w = g.worker::<Stuck>();
                g.set(&w, || Stuck { never: false });
            }
        }
        let run = g.launch().expect("accepted");
        let stuck = panic_text(|| {
            loop {
                run.wait();
            }
        });
        assert_eq!(stuck, "trame: deadlock: the leader waits and nothing can arrive");
        assert_eq!(run.join(), [Returned, Deadlock, Deadlock]);
    });
}

/// A panic in one worker aborts the run: the leader's join panics, naming the worker and its
/// message, and the waiter does not hang the test.
#[test]
fn panic_aborts_the_run_and_the_leader_panics() {
    within_10s(|| {
        let mut g = Topology::new();
        let a = g.worker::<Waiter>();
        g.set(&a, || Waiter {});
        let b = g.worker::<Boom>();
        g.set(&b, || Boom {});
        let run = g.launch().expect("accepted");
        let text = panic_text(move || {
            run.join();
        });
        assert_eq!(text, "trame: worker 1 panicked: intentional panic, for the abort test");
    });
}

/// A send on a route the topology does not have aborts the run, and the leader's join panics.
#[test]
fn send_without_a_route_aborts() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Stray>();
        g.set(&w, || Stray {});
        let run = g.launch().expect("accepted");
        let text = panic_text(move || {
            run.join();
        });
        assert!(text.starts_with("trame: worker 0 panicked: "), "{text}");
    });
}

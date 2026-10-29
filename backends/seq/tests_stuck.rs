//! Stuck runs and aborts: the run must end with the spec's results, never hang. Every test runs
//! under the 10 s guard.

extern crate std;

use super::tests::{Ping, panic_text, within_10s};
use crate::io;
use crate::run::Outcome::{Deadlock, Returned};
use crate::topology::{Context, LEADER, Topology};

fn quick(_cx: &Context) {}

fn wait_at_barrier(_cx: &Context) {
    io::barrier();
}

fn boom(_cx: &Context) {
    panic!("intentional panic, for the abort test");
}

fn stray(_cx: &Context) {
    // No route from a worker to the leader: the run aborts, naming the worker and the type.
    let _ = io::send(LEADER, Ping(0));
}

/// A worker returns while another waits at the barrier (Deadlock, spec.md 5). The waiter gets
/// `Deadlock`, whichever order the two workers are declared in.
#[test]
fn returned_worker_leaves_a_barrier_waiter_deadlocked() {
    within_10s(|| {
        let mut g = Topology::new();
        g.worker(quick);
        g.worker(wait_at_barrier);
        assert_eq!(g.launch().expect("accepted").join(), [Returned, Deadlock]);

        let mut g = Topology::new();
        g.worker(wait_at_barrier);
        g.worker(quick);
        assert_eq!(g.launch().expect("accepted").join(), [Deadlock, Returned]);
    });
}

/// Join called only after the run is stuck: the leader waits until its wait panics as stuck, so the
/// stall is there before join (spec.md 5, Deadlock), and join still returns.
#[test]
fn join_after_the_run_is_stuck_returns_deadlock() {
    within_10s(|| {
        let mut g = Topology::new();
        g.worker(quick);
        g.worker(wait_at_barrier);
        g.worker(wait_at_barrier);
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
/// message, and the waiter at the barrier does not hang the test.
#[test]
fn panic_aborts_the_run_and_the_leader_panics() {
    within_10s(|| {
        let mut g = Topology::new();
        g.worker(wait_at_barrier);
        g.worker(boom);
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
        g.worker(stray);
        let run = g.launch().expect("accepted");
        let text = panic_text(move || {
            run.join();
        });
        assert!(text.starts_with("trame: worker 0 panicked: "), "{text}");
    });
}

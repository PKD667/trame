//! Stuck runs and aborts: the run must end with the spec's results, never hang. Every test runs
//! under the 10 s guard.

extern crate std;

use super::tests::{within_10s, Ping};
use crate::io;
use crate::run::Outcome::{Abort, Deadlock, Returned};
use crate::topology::{Context, Topology, LEADER};
use std::thread;
use std::time::Duration;

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

/// Join called only after the run is stuck: the stall is already there, and join still returns.
#[test]
fn join_after_the_run_is_stuck_returns_deadlock() {
    within_10s(|| {
        let mut g = Topology::new();
        g.worker(quick);
        g.worker(wait_at_barrier);
        g.worker(wait_at_barrier);
        let run = g.launch().expect("accepted");
        thread::sleep(Duration::from_millis(50));
        assert_eq!(run.join(), [Returned, Deadlock, Deadlock]);
    });
}

/// A panic in one worker aborts every agent without a result, including one waiting at a barrier.
#[test]
fn panic_aborts_every_worker_without_a_result() {
    within_10s(|| {
        let mut g = Topology::new();
        g.worker(wait_at_barrier);
        g.worker(boom);
        assert_eq!(g.launch().expect("accepted").join(), [Abort, Abort]);
    });
}

/// A send on a route the topology does not have aborts the run.
#[test]
fn send_without_a_route_aborts() {
    within_10s(|| {
        let mut g = Topology::new();
        g.worker(stray);
        assert_eq!(g.launch().expect("accepted").join(), [Abort]);
    });
}

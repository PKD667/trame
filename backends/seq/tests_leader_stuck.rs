//! The leader blocked in `Run::wait` when nothing can arrive: the call panics (spec.md 5, Deadlock),
//! and does not hang. Every test runs under the 10 s guard.

extern crate std;

use super::tests::{Ping, panic_text, within_10s};
use crate::io;
use crate::topology::{Context, LEADER, Topology};

const STUCK: &str = "trame: deadlock: the leader waits and nothing can arrive";

fn quick(_cx: &Context) {}

fn wait_at_barrier(_cx: &Context) {
    io::barrier();
}

fn sends(_cx: &Context) {
    let _ = io::send(LEADER, Ping(7));
}

/// Two workers return and nothing is sent to the leader: its wait can never return.
#[test]
fn wait_after_every_worker_returned_panics() {
    within_10s(|| {
        let mut g = Topology::new();
        let a = g.worker(quick);
        let _b = g.worker(quick);
        g.route::<Ping>(a, LEADER, 1);
        let run = g.launch().expect("accepted");
        let text = panic_text(move || {
            loop {
                run.wait();
            }
        });
        assert_eq!(text, STUCK);
    });
}

/// A worker waits at the barrier that the other worker can never reach, since it returned.
#[test]
fn wait_with_a_worker_blocked_at_a_barrier_panics() {
    within_10s(|| {
        let mut g = Topology::new();
        g.worker(quick);
        g.worker(wait_at_barrier);
        let run = g.launch().expect("accepted");
        let text = panic_text(move || {
            loop {
                run.wait();
            }
        });
        assert_eq!(text, STUCK);
    });
}

/// A frame the leader can take is not stuck: the wait for it returns once the worker has sent.
#[test]
fn a_frame_from_a_worker_wakes_the_leader() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(sends);
        g.route::<Ping>(w, LEADER, 1);
        let run = g.launch().expect("accepted");
        let got = loop {
            if let Some(p) = run.recv::<Ping>() {
                break p;
            }
            run.wait();
        };
        assert_eq!(got, Ping(7));
    });
}

/// `Napper` waits 20 ms on its clock, then sends `Ping(7)` to the leader. While it waits, the leader's
/// `wait` must not be reported stuck: the deadline moves the worker (stuck.rs, earliest_deadline).
struct Napper {
    start: Option<crate::time::Reading>,
}

#[crate::process]
impl Napper {
    fn step(&mut self) -> crate::status::Status {
        let now = crate::time::now();
        match self.start {
            None => {
                self.start = Some(now);
                crate::status::Status::wait().until(now.after(core::time::Duration::from_millis(20)))
            }
            Some(_) => {
                let _ = io::send(LEADER, Ping(7));
                crate::status::Status::Done
            }
        }
    }
}

fn naps(_cx: &Context) {
    let r = crate::concurrent!(Napper { start: None };);
    assert_eq!(r, Ok(()));
}

/// A leader that waits while a worker has a pending deadline is not stuck: its wait returns once the
/// worker's time has passed and the worker has sent.
#[test]
fn a_pending_deadline_keeps_the_leader_waiting_not_stuck() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(naps);
        g.route::<Ping>(w, LEADER, 1);
        let run = g.launch().expect("accepted");
        let got = loop {
            if let Some(p) = run.recv::<Ping>() {
                break p;
            }
            run.wait();
        };
        assert_eq!(got, Ping(7));
    });
}

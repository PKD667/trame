//! `concurrent!` errors and deadlines: a failed process stops the others and its error is answered;
//! a `Status::wait().until` wakes at its deadline. Each test is one worker reporting to the leader (`Ev`).

extern crate std;

use super::tests::within_10s;
use super::tests_concurrent::{collect, enter, leave, report, Ev, Gate};
use crate::run::Outcome::Returned;
use crate::status::Status;
use crate::time::Reading;
use core::time::Duration;
use crate::topology::{Context, Topology, LEADER};

/// `Fails` (priority 0) returns an error at once. `Late` shares the gate and would report 500 if it
/// ran. It never does: no step starts once a process has failed. `concurrent!` answers the error.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Boom(pub u32);

struct Fails;

#[crate::process]
impl Fails {
    fn step(&mut self, _g: &mut Gate) -> Result<Status, Boom> {
        Err(Boom(7))
    }
}

struct Late;

#[crate::process]
impl Late {
    fn step(&mut self, g: &mut Gate) -> Result<Status, Boom> {
        enter(g);
        report(500);
        leave(g);
        Ok(Status::Done)
    }
}

fn fail_first(_cx: &Context) {
    let mut gate = Gate { inside: 0, open: false };
    let r = crate::concurrent!(Fails, Late; gate);
    report(match r {
        Err(Boom(n)) => n,
        Ok(()) => 0,
    });
}

#[test]
fn a_failed_process_stops_the_others_and_its_error_is_answered() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(fail_first);
        g.route::<Ev>(w, LEADER, 16);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 1), [7], "Late never ran; the error is Fails's");
        assert_eq!(run.join(), [Returned]);
    });
}

/// `Sleeper` waits until its clock reaches 20 ms after its first step, then reports whether at least
/// 20 ms passed. A deadline that never fires would leave the run stuck; it does not.
struct Sleeper {
    start: Option<Reading>,
}

#[crate::process]
impl Sleeper {
    fn step(&mut self) -> Status {
        let now = crate::time::now();
        match self.start {
            None => {
                self.start = Some(now);
                Status::wait().until(now.after(Duration::from_millis(20)))
            }
            Some(t0) => {
                let elapsed = now.since(t0);
                report(u32::from(elapsed >= Duration::from_millis(20)));
                Status::Done
            }
        }
    }
}

fn sleep_until(_cx: &Context) {
    let r = crate::concurrent!(Sleeper { start: None };);
    assert_eq!(r, Ok(()));
}

#[test]
fn wait_until_wakes_at_the_deadline() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(sleep_until);
        g.route::<Ev>(w, LEADER, 4);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 1), [1]);
        assert_eq!(run.join(), [Returned]);
    });
}

/// The leader joins at once, while the process waits for a deadline 20 ms away. The worker is not
/// stuck: time passes and it moves, so join must wait for it and must not answer Deadlock.
#[test]
fn join_waits_for_a_wait_until_deadline() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(sleep_until);
        g.route::<Ev>(w, LEADER, 4);
        let run = g.launch().expect("accepted");
        assert_eq!(run.join(), [Returned]);
    });
}

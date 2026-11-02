//! Errors and deadlines of steps: a step's error ends its worker as `Failed` and no step starts after
//! it; a `Status::wait().until` wakes at its deadline. Each test is one worker reporting to the
//! leader (`Ev`).

extern crate std;

use super::tests::within_10s;
use super::tests_steps::{Ev, collect, report};
use crate::run::Outcome::{Failed, Returned};
use crate::status::Status;
use crate::time::Reading;
use crate::topology::{LEADER, Topology};
use core::time::Duration;

/// `fails` (index 0) returns an error at its first step. `late` would report 500 and set `late_ran`
/// if it ran. It never does: no step starts once a step has failed. (Its report would also land in
/// the leader's `Ev` queue; `late_ran` is the check, since `join` consumes the run.)
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Boom(pub u32);

#[crate::state(steps(fails, late), error = Boom)]
struct Failing {
    late_ran: bool,
}

#[crate::step(Failing)]
fn fails() -> Result<Status, Boom> {
    Err(Boom(7))
}

#[crate::step(Failing)]
fn late(late_ran: &mut bool) -> Result<Status, Boom> {
    *late_ran = true;
    report(500);
    Ok(Status::Done)
}

#[test]
fn a_failed_step_ends_the_worker_and_its_error_is_kept() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Failing>();
        g.set(&w, || Failing { late_ran: false });
        g.route::<Ev>(w.id(), LEADER, 16);
        let run = g.launch().expect("accepted");
        assert_eq!(run.join(), [Failed]);
        assert_eq!(w.error(), Some(Boom(7)));
        assert!(!w.take().late_ran, "a step started after another failed");
    });
}

/// `sleeper` waits until its clock reaches 20 ms after its first step, then reports whether at least
/// 20 ms passed. A deadline that never fires would leave the run stuck; it does not.
#[crate::state(steps(sleeper))]
struct Sleeper {
    start: Option<Reading>,
}

#[crate::step(Sleeper)]
fn sleeper(start: &mut Option<Reading>) -> Status {
    let now = crate::time::now();
    match *start {
        None => {
            *start = Some(now);
            Status::wait().until(now.after(Duration::from_millis(20)))
        }
        Some(t0) => {
            let elapsed = now.since(t0);
            report(u32::from(elapsed >= Duration::from_millis(20)));
            Status::Done
        }
    }
}

#[test]
fn wait_until_wakes_at_the_deadline() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Sleeper>();
        g.set(&w, || Sleeper { start: None });
        g.route::<Ev>(w.id(), LEADER, 4);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 1), [1]);
        assert_eq!(run.join(), [Returned]);
    });
}

/// The leader joins at once, while the step waits for a deadline 20 ms away. The worker is not
/// stuck: time passes and it moves, so join must wait for it and must not answer Deadlock.
#[test]
fn join_waits_for_a_wait_until_deadline() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Sleeper>();
        g.set(&w, || Sleeper { start: None });
        g.route::<Ev>(w.id(), LEADER, 4);
        let run = g.launch().expect("accepted");
        assert_eq!(run.join(), [Returned]);
    });
}

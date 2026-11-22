//! Errors of steps: a step's error ends its worker as `Failed` and no step starts after it. Each test is one
//! worker reporting to the leader (`Ev`).

extern crate std;

use super::tests::within_10s;
use super::tests_steps::{Ev, report};
use crate::run::Outcome::Failed;
use crate::status::Status;
use crate::topology::{Leader, Topology};

/// `fails` (index 0) returns an error at its first step. `late` would report 500 and set `late_ran`
/// if it ran. It never does: no step starts once a step has failed. (Its report would also land in
/// the leader's `Ev` queue; `late_ran` is the check, since `join` consumes the run.)
#[derive(Debug, PartialEq, Eq, crate::Plain)]
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
        g.route::<Ev>(w.id(), Leader, 16);
        let run = g.launch().expect("accepted");
        assert_eq!(run.join(), [Failed]);
        assert_eq!(w.error(), Some(Boom(7)));
        assert!(!w.take().late_ran, "a step started after another failed");
    });
}

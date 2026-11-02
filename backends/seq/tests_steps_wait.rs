//! `Status::wait()`: a step that waits on its fields is woken by the end of a step that writes one.
//! The test is one worker reporting to the leader (`Ev`).

extern crate std;

use super::tests::within_10s;
use super::tests_steps::{Ev, Gate, collect, enter, leave, report};
use crate::run::Outcome::Returned;
use crate::status::Status;
use crate::topology::{LEADER, Topology};

/// `waiter` (index 0) waits on the gate until it is open. `opener` opens it and ends. Ending a step
/// that wrote the gate wakes the step that uses it, so `waiter` runs again after `opener`. A lost
/// wake-up would leave the run stuck, and the 10 s guard would fail it.
#[crate::state(steps(waiter, opener))]
struct Gated {
    gate: Gate,
}

#[crate::step(Gated)]
fn waiter(gate: &mut Gate) -> Status {
    enter(gate);
    let s = if gate.open {
        report(2);
        Status::Done
    } else {
        report(1);
        Status::wait()
    };
    leave(gate);
    s
}

#[crate::step(Gated)]
fn opener(gate: &mut Gate) -> Status {
    enter(gate);
    gate.open = true;
    report(3);
    leave(gate);
    Status::Done
}

#[test]
fn a_waiter_on_fields_is_woken_after_the_other_steps() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Gated>();
        g.set(&w, || Gated { gate: Gate { inside: 0, open: false } });
        g.route::<Ev>(w.id(), LEADER, 16);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 3), [1, 3, 2]);
        assert_eq!(run.join(), [Returned]);
    });
}

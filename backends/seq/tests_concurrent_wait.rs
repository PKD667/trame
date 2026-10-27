//! `Status::wait()`: a process that waits on its cells is woken by the end of a step that shares one.
//! Each test is one worker reporting to the leader (`Ev`).

extern crate std;

use super::tests::within_10s;
use super::tests_concurrent::{collect, enter, leave, report, Ev, Gate};
use crate::run::Outcome::Returned;
use crate::status::Status;
use crate::topology::{Context, Topology, LEADER};

/// `Waiter` (priority 0) waits on cells until the gate opens. `Opener` opens it and ends. Ending a
/// step wakes the process that shares a cell with it, so `Waiter` runs again after `Opener`. A lost
/// wake-up would leave the run stuck, and the 10 s guard would fail it.
struct Waiter;

#[crate::process]
impl Waiter {
    fn step(&mut self, g: &mut Gate) -> Status {
        enter(g);
        let s = if g.open {
            report(2);
            Status::Done
        } else {
            report(1);
            Status::wait()
        };
        leave(g);
        s
    }
}

struct Opener;

#[crate::process]
impl Opener {
    fn step(&mut self, g: &mut Gate) -> Status {
        enter(g);
        g.open = true;
        report(3);
        leave(g);
        Status::Done
    }
}

fn wake_on_cells(_cx: &Context) {
    let mut gate = Gate { inside: 0, open: false };
    let r = crate::concurrent!(Waiter, Opener; gate);
    assert_eq!(r, Ok(()));
}

#[test]
fn a_waiter_on_cells_is_woken_after_the_other_steps() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(wake_on_cells);
        g.route::<Ev>(w, LEADER, 16);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 3), [1, 3, 2]);
        assert_eq!(run.join(), [Returned]);
    });
}


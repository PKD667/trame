//! `Status::wait().room`: a step whose send answers `Full` waits for room, and the leader's take wakes it.
//! The test is one worker; the leader takes the frames and reads the worker's reports (`Ev`).

extern crate std;

use super::tests::within_10s;
use super::tests_steps::{Ev, collect};
use crate::io::{self, Full};
use crate::run::Outcome::Returned;
use crate::status::Status;
use crate::topology::{Leader, Topology};

/// Sends three values to the leader over a route of capacity 1. The second send answers `Full` until
/// the leader takes the first, so the step waits for room, and the leader's take must wake it.
#[crate::state(steps(pump))]
struct Pump {
    sent: u32,
}

#[crate::step(Pump)]
fn pump(sent: &mut u32) -> Status {
    match io::send(Leader, Ev { v: *sent }) {
        Ok(()) => {
            *sent += 1;
            if *sent == 3 { Status::Done } else { Status::Progress }
        }
        Err(Full) => Status::wait().room::<Ev>(Leader),
    }
}

#[test]
fn a_wait_for_room_resumes_when_the_leader_takes_a_frame() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Pump>();
        g.set(&w, || Pump { sent: 0 });
        g.route::<Ev>(w.id(), Leader, 1);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 3), [0, 1, 2]);
        assert_eq!(run.join(), [Returned]);
    });
}

//! `Status::wait().room`: a process whose send answers `Full` waits for room, and the leader's take wakes it.
//! Each test is one worker; the leader takes the frames and reads the worker's reports (`Ev`).

extern crate std;

use super::tests_concurrent::{Ev, Gate, collect};
use crate::io::{self, Full};
use crate::run::Outcome::Returned;
use crate::status::Status;
use crate::topology::{Context, LEADER, Topology};

use super::tests::within_10s;

/// Sends three values to the leader over a route of capacity 1. The second send answers `Full` until
/// the leader takes the first, so the process waits for room, and the leader's take must wake it.
struct Pump {
    sent: u32,
}

#[crate::process]
impl Pump {
    fn step(&mut self, _g: &mut Gate) -> Status {
        match io::send(LEADER, Ev { v: self.sent }) {
            Ok(()) => {
                self.sent += 1;
                if self.sent == 3 { Status::Done } else { Status::Progress }
            }
            Err(Full) => Status::wait().room::<Ev>(LEADER),
        }
    }
}

fn pump_worker(_cx: &Context) {
    let mut gate = Gate { inside: 0, open: false };
    let r = crate::concurrent!(Pump { sent: 0 }; gate);
    assert_eq!(r, Ok(()));
}

#[test]
fn a_wait_for_room_resumes_when_the_leader_takes_a_frame() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(pump_worker);
        g.route::<Ev>(w, LEADER, 1);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 3), [0, 1, 2]);
        assert_eq!(run.join(), [Returned]);
    });
}

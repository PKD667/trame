//! `concurrent!` that cannot finish: a process waits for a frame that no leader will send. The run
//! must end with `Deadlock` (spec.md 5, Deadlock), not hang. Also many rounds of two workers, each
//! waiting on frames from the leader, so that the baton is passed between idle workers in many orders.

extern crate std;

use super::tests::within_10s;
use super::tests_concurrent::{collect, report, Ev, Gate};
use super::tests_concurrent_io::Job;
use crate::io;
use crate::run::Outcome::{Deadlock, Returned};
use crate::status::Status;
use crate::topology::{Context, Topology, LEADER};

/// Owns `Job`, and waits for one. Reports the id of each job it gets.
struct Await;

#[crate::process(receives = Job)]
impl Await {
    fn step(&mut self, _g: &mut Gate) -> Status {
        match io::recv::<Job>() {
            Some(j) => {
                report(j.id);
                Status::Done
            }
            None => Status::wait().frame(),
        }
    }
}

fn await_worker(_cx: &Context) {
    let mut gate = Gate { inside: 0, open: false };
    let r = crate::concurrent!(Await; gate);
    assert_eq!(r, Ok(()));
}

fn lonely(_cx: &Context) {
    let mut gate = Gate { inside: 0, open: false };
    let r = crate::concurrent!(Await; gate);
    report(u32::from(r.is_ok()));
}

/// No frame comes, and the leader joins at once: nothing can move, so the worker is deadlocked.
#[test]
fn a_frame_wait_that_no_leader_can_satisfy_is_deadlock() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(lonely);
        g.route::<Ev>(w, LEADER, 4);
        g.route::<Job>(LEADER, w, 4);
        let run = g.launch().expect("accepted");
        assert_eq!(run.join(), [Deadlock]);
    });
}

/// Two workers each wait for one job. The leader sends one job to each, in alternating order, many
/// times. Every round must finish with both workers returned and every job reported.
#[test]
fn two_idle_workers_all_finish_over_many_rounds() {
    within_10s(|| {
        for round in 0..200_u32 {
            let mut g = Topology::new();
            let a = g.worker(await_worker);
            let b = g.worker(await_worker);
            for w in [a, b] {
                g.route::<Ev>(w, LEADER, 4);
                g.route::<Job>(LEADER, w, 4);
            }
            let run = g.launch().expect("accepted");
            let (first, second) = if round % 2 == 0 { (a, b) } else { (b, a) };
            run.send(first, Job { id: 1 }).expect("room");
            run.send(second, Job { id: 2 }).expect("room");
            let mut got = collect(&run, 2);
            got.sort_unstable();
            assert_eq!(got, [1, 2], "round {round}");
            assert_eq!(run.join(), [Returned, Returned], "round {round}");
        }
    });
}

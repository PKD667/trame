//! `concurrent!` through the sequential backend: priority, the cells processes share, and the wait
//! conditions. Each test is one worker that runs a `concurrent!` and reports what its steps do to the
//! leader, as `Ev` frames, so the order is observed by the leader in the order it happened. Compiled
//! only in a checked build, where `concurrent!` is allowed (see mod.rs).

extern crate std;

use super::tests::{await_frame, within_10s};
use crate::io;
use crate::run::Outcome::Returned;
use crate::run::Run;
use crate::status::Status;
use crate::topology::{Context, LEADER, Topology};
use alloc::vec::Vec;

/// One observation of a step, reported by the worker to the leader.
#[crate::message]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Ev {
    pub v: u32,
}

/// A cell shared by the processes of one `concurrent!`. `inside` counts the steps that hold it.
pub(super) struct Gate {
    pub inside: u32,
    pub open: bool,
}

/// Report one observation to the leader. Each send is a worker effect, so it retries on `Full`.
pub(super) fn report(v: u32) {
    while io::send(LEADER, Ev { v }).is_err() {}
}

/// Take a step's hold on the gate: no other step may hold it now (spec.md 5.3, Start).
pub(super) fn enter(g: &mut Gate) {
    g.inside += 1;
    assert_eq!(g.inside, 1, "two steps hold one cell at once");
}

pub(super) fn leave(g: &mut Gate) {
    g.inside -= 1;
}

/// The leader's view: the next `n` observations, in order.
pub(super) fn collect(run: &Run, n: usize) -> Vec<u32> {
    (0..n).map(|_| await_frame::<Ev>(run).v).collect()
}

/// Process `Hi` (priority 0) takes `left + 1` steps and reports 10 + its countdown.
struct Hi {
    left: u32,
}

#[crate::process]
impl Hi {
    fn step(&mut self, g: &mut Gate) -> Status {
        enter(g);
        report(10 + self.left);
        let s = if self.left == 0 {
            Status::Done
        } else {
            self.left -= 1;
            Status::Progress
        };
        leave(g);
        s
    }
}

/// Process `Lo` (priority 1) reports 20 + its countdown, on the same gate as `Hi`.
struct Lo {
    left: u32,
}

#[crate::process]
impl Lo {
    fn step(&mut self, g: &mut Gate) -> Status {
        enter(g);
        report(20 + self.left);
        let s = if self.left == 0 {
            Status::Done
        } else {
            self.left -= 1;
            Status::Progress
        };
        leave(g);
        s
    }
}

fn priority(_cx: &Context) {
    let mut gate = Gate { inside: 0, open: false };
    let r = crate::concurrent!(Hi { left: 2 }, Lo { left: 1 }; gate);
    assert_eq!(r, Ok(()));
}

/// A process that is always ready should run before every later one, and the two never overlap.
/// Hi runs its three steps first (12, 11, 10), then Lo its two (21, 20).
#[test]
fn priority_order_and_no_overlap_on_a_shared_cell() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(priority);
        g.route::<Ev>(w, LEADER, 16);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 5), [12, 11, 10, 21, 20]);
        assert_eq!(run.join(), [Returned]);
    });
}

//! The steps of one worker through the sequential backend: which step starts, and the field they
//! share. Each test is one worker whose steps report what they do to the leader, as `Ev` frames, so
//! the leader sees the order in which things happened. Compiled only in a checked build, where
//! `#[trame::state]` is allowed (see mod.rs).

extern crate std;

use super::tests::{await_frame, within_10s};
use crate::io;
use crate::run::Outcome::Returned;
use crate::run::Run;
use crate::status::Status;
use crate::topology::{Leader, Topology};
use alloc::vec::Vec;

/// One observation of a step, reported by the worker to the leader.
#[crate::message]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Ev {
    pub v: u32,
}

/// A field shared by the steps of one worker. `inside` counts the steps that hold it.
#[derive(crate::Plain)]
pub(super) struct Gate {
    pub inside: u32,
    pub open: bool,
}

/// Report one observation to the leader. The routes of these tests have room for every report, so a
/// send never answers `Full`.
pub(super) fn report(v: u32) {
    assert!(io::send(Leader, Ev { v }).is_ok(), "a report did not fit");
}

/// Take a step's hold on the gate: no other step may hold it now (spec.md 5.3, Start).
pub(super) fn enter(g: &mut Gate) {
    g.inside += 1;
    assert_eq!(g.inside, 1, "two steps hold one field at once");
}

pub(super) fn leave(g: &mut Gate) {
    g.inside -= 1;
}

/// The leader's view: the next `n` observations, in order.
pub(super) fn collect(run: &Run, n: usize) -> Vec<u32> {
    (0..n).map(|_| await_frame::<Ev>(run).v).collect()
}

/// One step of a counting process: hold the gate, report `base` + the countdown, count down, and
/// finish at 0.
fn tick(gate: &mut Gate, left: &mut u32, base: u32) -> Status {
    enter(gate);
    report(base + *left);
    let s = if *left == 0 {
        Status::Done
    } else {
        *left -= 1;
        Status::Progress
    };
    leave(gate);
    s
}

/// `lo` (declared first, weight 0) and `hi` (weight 1) share the gate: while `hi` can start, `lo`
/// does not.
#[crate::state(steps(lo, hi))]
struct Weighted {
    gate: Gate,
    lo_left: u32,
    hi_left: u32,
}

#[crate::step(Weighted)]
fn lo(gate: &mut Gate, lo_left: &mut u32) -> Status {
    tick(gate, lo_left, 20)
}

#[crate::step(Weighted, weight = 1.0)]
fn hi(gate: &mut Gate, hi_left: &mut u32) -> Status {
    tick(gate, hi_left, 10)
}

/// A step of positive weight runs before every step of weight 0, and the two never overlap on their
/// shared field. `hi` takes its three steps first (12, 11, 10), then `lo` its two (21, 20).
#[test]
fn positive_weight_runs_first_and_no_overlap_on_a_shared_field() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Weighted>();
        g.set(&w, || Weighted { gate: Gate { inside: 0, open: false }, lo_left: 1, hi_left: 2 });
        g.route::<Ev>(w.id(), Leader, 16);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 5), [12, 11, 10, 21, 20]);
        assert_eq!(run.join(), [Returned]);
    });
}

/// `first` and `second`, both weight 0, share the gate: the one started least recently goes next.
#[crate::state(steps(first, second))]
struct Fair {
    gate: Gate,
    first_left: u32,
    second_left: u32,
}

#[crate::step(Fair)]
fn first(gate: &mut Gate, first_left: &mut u32) -> Status {
    tick(gate, first_left, 10)
}

#[crate::step(Fair)]
fn second(gate: &mut Gate, second_left: &mut u32) -> Status {
    tick(gate, second_left, 20)
}

/// Steps of weight 0 take turns, the lower index first (sched.rs, `choose`): 12, 21, 11, 20, 10.
#[test]
fn steps_of_weight_zero_take_turns_on_a_shared_field() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Fair>();
        g.set(&w, || Fair { gate: Gate { inside: 0, open: false }, first_left: 2, second_left: 1 });
        g.route::<Ev>(w.id(), Leader, 16);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 5), [12, 21, 11, 20, 10]);
        assert_eq!(run.join(), [Returned]);
    });
}

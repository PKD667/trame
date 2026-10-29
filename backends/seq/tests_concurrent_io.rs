//! `concurrent!` and frames: a process wakes on a frame of a type it owns only, cannot receive a type
//! it does not own, and leaves the frames it does not own queued for the worker. Also `Status::wait().room`.
//! Each test is one worker; the leader sends frames and reads the worker's reports (`Ev`).

extern crate std;

use super::tests::within_10s;
use super::tests_concurrent::{Ev, Gate, collect, report};
use crate::io;
use crate::run::Outcome::Returned;
use crate::status::Status;
use crate::topology::{Context, LEADER, Topology};

#[crate::message]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Job {
    pub id: u32,
}

#[crate::message]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Other {
    pub id: u32,
}

/// Owns `Job`. Each step takes a `Job` if there is one, then reports `100 + count`. The take comes
/// first so step 1 cannot see a `Job` the leader sends after reading 101: the program has one
/// execution. An `Other` frame is not an offer to it, so it waits until the `Job` arrives.
struct Frames {
    count: u32,
}

#[crate::process(receives = Job)]
impl Frames {
    fn step(&mut self, _g: &mut Gate) -> Status {
        self.count += 1;
        let job = io::recv::<Job>();
        report(100 + self.count);
        match job {
            Some(_) => Status::Done,
            None => Status::wait().frame(),
        }
    }
}

fn frames_worker(_cx: &Context) {
    let mut gate = Gate { inside: 0, open: false };
    let r = crate::concurrent!(Frames { count: 0 }; gate);
    assert_eq!(r, Ok(()));
}

/// A `Status::wait().frame` wakes on an owned type only: an `Other` frame leaves the process asleep.
#[test]
fn a_frame_wait_wakes_on_an_owned_type_only() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(frames_worker);
        g.route::<Ev>(w, LEADER, 16);
        g.route::<Job>(LEADER, w, 4);
        g.route::<Other>(LEADER, w, 4);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 1), [101], "the first step waits for a job");
        // That `Other` does not wake the process is programs/frame_other's check (a step count);
        // here it only must stay queued while the `Job` is taken.
        run.send(w, Other { id: 1 }).expect("room");
        run.send(w, Job { id: 2 }).expect("room");
        assert_eq!(collect(&run, 1), [102]);
        assert_eq!(run.join(), [Returned]);
    });
}

/// Owns `Job` and tries `Other` first. The unowned `Other` answers `None` and stays queued (a report
/// of 900 would mean it was taken). After `concurrent!`, the worker receives it: the queued frames
/// become the worker's again.
struct Peek;

#[crate::process(receives = Job)]
impl Peek {
    fn step(&mut self, _g: &mut Gate) -> Status {
        if let Some(o) = io::recv::<Other>() {
            report(900 + o.id);
        }
        match io::recv::<Job>() {
            Some(j) => {
                report(j.id);
                Status::Done
            }
            None => Status::wait().frame(),
        }
    }
}

fn peek_worker(_cx: &Context) {
    let mut gate = Gate { inside: 0, open: false };
    let r = crate::concurrent!(Peek; gate);
    assert_eq!(r, Ok(()));
    match io::recv::<Other>() {
        Some(o) => report(800 + o.id),
        None => report(0),
    }
}

#[test]
fn a_process_cannot_receive_a_type_it_does_not_own_and_its_frames_stay_queued() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(peek_worker);
        g.route::<Ev>(w, LEADER, 16);
        g.route::<Job>(LEADER, w, 4);
        g.route::<Other>(LEADER, w, 4);
        let run = g.launch().expect("accepted");
        // Other is sent first, so when the Job is there the Other already is too.
        run.send(w, Other { id: 3 }).expect("room");
        run.send(w, Job { id: 5 }).expect("room");
        assert_eq!(collect(&run, 2), [5, 803]);
        assert_eq!(run.join(), [Returned]);
    });
}

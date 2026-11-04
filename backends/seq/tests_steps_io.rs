//! Steps and frames: a step wakes on a frame of a type it receives only, cannot receive a type it does
//! not receive, and leaves the frames it does not receive queued for the step that does. Each test is
//! one worker; the leader sends frames and reads the worker's reports (`Ev`).

extern crate std;

use super::tests::within_10s;
use super::tests_steps::{Ev, collect, report};
use crate::io;
use crate::run::Outcome::Returned;
use crate::status::Status;
use crate::topology::{Leader, Topology};

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

/// Receives `Job`. Each call takes a `Job` if there is one, then reports `100 + count`. The take comes
/// first so call 1 cannot see a `Job` the leader sends after reading 101: the program has one
/// execution. An `Other` frame is not an offer to it, so it waits until the `Job` arrives.
#[crate::state(steps(frames))]
struct Framed {
    count: u32,
}

#[crate::step(Framed, receives = Job)]
fn frames(count: &mut u32) -> Status {
    *count += 1;
    let job = io::recv::<Job>();
    report(100 + *count);
    match job {
        Some(_) => Status::Done,
        None => Status::wait().frame(),
    }
}

/// A `Status::wait().frame` wakes on a received type only: an `Other` frame leaves the step asleep.
#[test]
fn a_frame_wait_wakes_on_a_received_type_only() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Framed>();
        g.set(&w, || Framed { count: 0 });
        g.route::<Ev>(w.id(), Leader, 16);
        g.route::<Job>(Leader, w.id(), 4);
        g.route::<Other>(Leader, w.id(), 4);
        let run = g.launch().expect("accepted");
        assert_eq!(collect(&run, 1), [101], "the first call waits for a job");
        // That `Other` does not wake the step is programs/frame_other's check (a call count);
        // here it only must stay queued while the `Job` is taken.
        run.send(w.id(), Other { id: 1 }).expect("room");
        run.send(w.id(), Job { id: 2 }).expect("room");
        assert_eq!(collect(&run, 1), [102]);
        assert_eq!(run.join(), [Returned]);
    });
}

/// `peek` receives `Job` and tries `Other` first. The `Other` is not its type, so the call answers
/// `None` and the frame stays queued (a report of 900 would mean it was taken). `later` receives
/// `Other`, and starts to look only once `peek` has taken the `Job`: the queued frame is then its.
#[crate::state(steps(peek, later))]
struct Peeking {
    got_job: bool,
}

#[crate::step(Peeking, receives = Job)]
fn peek(got_job: &mut bool) -> Status {
    if let Some(o) = io::recv::<Other>() {
        report(900 + o.id);
    }
    match io::recv::<Job>() {
        Some(j) => {
            report(j.id);
            *got_job = true;
            Status::Done
        }
        None => Status::wait().frame(),
    }
}

#[crate::step(Peeking, receives = Other)]
fn later(got_job: &bool) -> Status {
    if !*got_job {
        return Status::wait();
    }
    match io::recv::<Other>() {
        Some(o) => {
            report(800 + o.id);
            Status::Done
        }
        None => Status::wait().frame(),
    }
}

#[test]
fn a_step_cannot_receive_a_type_it_does_not_receive_and_its_frames_stay_queued() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Peeking>();
        g.set(&w, || Peeking { got_job: false });
        g.route::<Ev>(w.id(), Leader, 16);
        g.route::<Job>(Leader, w.id(), 4);
        g.route::<Other>(Leader, w.id(), 4);
        let run = g.launch().expect("accepted");
        // Other is sent first, so when the Job is there the Other already is too.
        run.send(w.id(), Other { id: 3 }).expect("room");
        run.send(w.id(), Job { id: 5 }).expect("room");
        assert_eq!(collect(&run, 2), [5, 803]);
        assert_eq!(run.join(), [Returned]);
    });
}

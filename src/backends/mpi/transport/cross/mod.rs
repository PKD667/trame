// Routes between workers of two hosts (CONTRACT.md, F3, F5). Frames go rank to rank by two-sided MPI
// on `Host::cross`, a duplicate of the world, with no thread between: the experiment mpi_cross_host
// measured a relay thread at 49 to 50 us one way for 8 bytes against 30 us rank to rank, on two
// machines (transport/README.md, Routes between hosts).
//
// A route is a lane. Its frames travel with MPI tag 2 lane, its credits with 2 lane + 1. The lane of
// a route is its position, by F4 tag, among the routes with the same sender and receiver, so the two
// hosts number it alike once they agree on their declarations (`connect`). MPI matches messages of one
// sender, tag and communicator in the order they were sent, which keeps T1.
//
// Receiver. It keeps `cap` receive buffers, each with a posted MPI_Irecv: buffer `i mod cap` holds
// frame i, for i from C, the frames consumed, to C + cap. Taking frame C copies it out, posts the
// receive of frame C + cap in its buffer, and sends C + 1 back as a credit.
//
// Sender. It knows C only from credits: `c`, the largest it received, is at most the receiver's C.
// Frame P is sent only when P - c < cap, so it always finds a posted receive, and a lossless send
// answers Full exactly when P - c = cap. That is INTERFACE.md's Full with U, the frames taken whose credit
// has not arrived (Release). A lossy send never waits: with no room, it keeps the frame as pending,
// replacing any older pending one (a Drop of a frame that is not the newest), and sends it once a
// credit makes room.
//
// Wake (S3). An agent of a worker with lanes never sleeps on the futex: it polls its lanes between
// checks of its epoch, as the experiment's mixed_flag mode did, which cost nothing measurable on the
// frames' latency. Every arrival of a frame or of a credit that a test observes moves the worker's
// epoch, whoever observed it, so a waiter that read the epoch before cannot miss it.
//
// Counts (roles/detect.rs). Each end counts in the window, in its route's P and C words, the messages it
// sent and those it saw arrive, frames and credits alike. A credit counts as sent when it falls due,
// since it is sent at a later poll however long the end idles; a frame counts as arrived when a test
// observes it, at any slot, not only the head's, so a frame that waits behind an untaken head is
// counted. An end that has ended keeps testing its posted receives until the nonbarrier completes.
// Then a message in flight is exactly a difference of the sums of the two counts over every end.
//
// End. MPI_Finalize needs every message complete. `quiesce`, before it on every rank: complete this
// rank's sends, which always match a posted receive; then a nonbarrier, MPI_Ibarrier on `cross`,
// progressing all the while; once it completes no rank has a send outstanding, and the posted
// receives are cancelled. A pending lossy frame is dropped; frames sent to a worker that has ended are
// not taken (federation: a route is lossless while both ends run), though they are counted.

extern crate std;

use std::vec::Vec;

use mpi::ffi;
use mpi::traits::*;

use crate::backends::mpi::memory::layout::{LINE, Layout, Remote, RouteLayout};
use crate::backends::mpi::memory::shm::Window;
use crate::backends::mpi::transport::hosts::host;

mod calls;
mod connect;
mod count;
mod receiver;
mod sender;

use calls::{null, ok, test, wait};
use connect::answer_connects;
use count::Count;

pub(crate) use connect::{connect, global};
pub(crate) use receiver::In;
pub(crate) use sender::Out;

pub(crate) enum Lane {
    Out(Out),
    In(In),
}

/// The lanes of the endpoint of rank `me`, by route: one per route between hosts that it sends or
/// receives. Posts the receives. Allocates: called at launch, before any step.
pub(crate) fn lanes(lay: &Layout, me: u32, win: &Window) -> Vec<Option<Lane>> {
    let mut out = Vec::with_capacity(lay.routes.len());
    for (rid, r) in lay.routes.iter().enumerate() {
        out.push(r.remote.filter(|_| r.sent_by(me) || r.received_by(me)).map(|x| lane(rid, r, x, win)));
    }
    out
}

fn lane(rid: usize, r: &RouteLayout, x: Remote, win: &Window) -> Lane {
    let (sent, got) = (Count::at(win, r.base), Count::at(win, r.base + LINE));
    let comm = host().cross.as_raw();
    if x.outgoing {
        Lane::Out(Out::open(rid, r, x, comm, sent, got))
    } else {
        Lane::In(In::open(rid, r, x, comm, sent, got))
    }
}

impl Lane {
    /// Progress: take credits, test the head. Answers whether anything arrived.
    pub(crate) fn poll(&mut self) -> bool {
        match self {
            Lane::Out(o) => o.poll(),
            Lane::In(i) => {
                let head = i.head().1;
                i.observe() || head
            }
        }
    }

    fn drain(&mut self) -> bool {
        match self {
            Lane::Out(o) => o.drain(),
            Lane::In(i) => i.drain(),
        }
    }

    fn close(&mut self) {
        match self {
            Lane::Out(o) => o.close(),
            Lane::In(i) => i.close(),
        }
    }
}

/// Before MPI_Finalize, on every rank of the world (see the module comment). `lanes` are this rank's.
pub(crate) fn quiesce(lanes: &mut [Option<Lane>]) {
    let idle = || std::thread::sleep(std::time::Duration::from_micros(20));
    let all = |lanes: &mut [Option<Lane>]| lanes.iter_mut().flatten().fold(true, |a, l| l.drain() && a);
    while !all(lanes) {
        idle();
    }
    let mut bar = null();
    // SAFETY: `cross` is a communicator of every rank of the world, and every rank calls this once,
    // in `finalize` (F5; Foreign calls).
    ok(unsafe { ffi::MPI_Ibarrier(host().cross.as_raw(), &mut bar) }, "Ibarrier");
    let mut answers = Vec::new();
    // A leader takes part in the detector's rounds until every leader has ended (roles/detect.rs).
    if host().peers.is_some() {
        while crate::backends::mpi::roles::detect::round(true) != crate::backends::mpi::roles::detect::Verdict::Over {
            answer_connects(&mut answers);
            idle();
        }
    }
    while !test(&mut bar) {
        all(lanes);
        answer_connects(&mut answers);
        idle();
    }
    for (_, r) in &mut answers {
        wait(r);
    }
    for l in lanes.iter_mut().flatten() {
        l.close();
    }
}

// The receiving end of a lane (Receiver, in mod.rs): the frames in, the credits out.

extern crate std;

use std::boxed::Box;

use mpi::ffi;

use crate::backends::mpi::memory::layout::{Remote, RouteLayout};
use crate::backends::mpi::transport::cross::calls::{cancel, irecv, isend, null, test};
use crate::backends::mpi::transport::cross::count::Count;

/// The receiving end of a lane.
pub(crate) struct In {
    comm: ffi::MPI_Comm,
    peer: i32,
    lane: i32,
    size: usize,
    cap: u64,
    /// Frames consumed.
    c: u64,
    bufs: Box<[u8]>,
    reqs: Box<[ffi::MPI_Request]>,
    /// Whether the frame of buffer i has arrived (its request completed, now null).
    arrived: Box<[bool]>,
    credit: Box<u64>,
    credit_req: ffi::MPI_Request,
    /// A credit to send once the last one has gone.
    due: bool,
    /// Credits sent (from when they fall due), frames arrived (`Counts`).
    sent: Count,
    got: Count,
}

impl In {
    /// The end of route `r` that receives, with the receive of every slot posted.
    pub(super) fn open(r: &RouteLayout, x: Remote, comm: ffi::MPI_Comm, sent: Count, got: Count) -> In {
        let n = r.size;
        let cap = r.cap as usize;
        let bufs = std::vec![0u8; n * cap].into_boxed_slice();
        let reqs = std::vec![null(); cap].into_boxed_slice();
        let mut i = In {
            comm,
            peer: x.world,
            lane: x.lane,
            size: n,
            cap: r.cap,
            c: 0,
            bufs,
            reqs,
            arrived: std::vec![false; cap].into_boxed_slice(),
            credit: Box::new(0),
            credit_req: null(),
            due: false,
            sent,
            got,
        };
        for s in 0..cap {
            i.post(s);
        }
        i
    }

    fn post(&mut self, s: usize) {
        let at = self.bufs[s * self.size..].as_mut_ptr();
        irecv(at, self.size, self.peer, 2 * self.lane, self.comm, &mut self.reqs[s]);
    }

    /// Send the credit if one is due and the last has gone.
    fn credit(&mut self) {
        if self.due && test(&mut self.credit_req) {
            *self.credit = self.c;
            isend(
                (&*self.credit as *const u64).cast(),
                8,
                self.peer,
                2 * self.lane + 1,
                self.comm,
                &mut self.credit_req,
            );
            self.due = false;
        }
    }

    /// Frames consumed: the index of the head.
    pub(crate) fn consumed(&self) -> u64 {
        self.c
    }

    /// Whether frame C has arrived; the second value says whether it arrived just now.
    pub(crate) fn head(&mut self) -> (bool, bool) {
        self.credit();
        let s = (self.c % self.cap) as usize;
        if self.arrived[s] {
            return (true, false);
        }
        let now = test(&mut self.reqs[s]);
        self.arrived[s] = now;
        if now {
            self.got.add();
        }
        (now, now)
    }

    /// Observe every frame that has arrived, at any slot (`Counts`). Answers whether one did.
    pub(super) fn observe(&mut self) -> bool {
        let mut any = false;
        for s in 0..self.reqs.len() {
            if !self.arrived[s] && test(&mut self.reqs[s]) {
                self.arrived[s] = true;
                self.got.add();
                any = true;
            }
        }
        any
    }

    /// Take frame C into `out` if it has arrived: its buffer gets the receive of frame C + cap, and a
    /// credit goes back. The second value as `head`'s.
    pub(crate) fn take(&mut self, out: &mut [u8]) -> (Option<u64>, bool) {
        let (here, now) = self.head();
        if !here {
            return (None, now);
        }
        let s = (self.c % self.cap) as usize;
        out[..self.size].copy_from_slice(&self.bufs[s * self.size..(s + 1) * self.size]);
        self.arrived[s] = false;
        self.post(s);
        let index = self.c;
        self.c += 1;
        if !self.due {
            self.sent.add();
        }
        self.due = true;
        self.credit();
        (Some(index), now)
    }

    /// Send the credit that is due, which counts as sent already; send no more after it; observe
    /// the frames that arrive (`Counts`). Answers whether the last credit is complete.
    pub(super) fn drain(&mut self) -> bool {
        self.credit();
        self.observe();
        !self.due && test(&mut self.credit_req)
    }

    pub(super) fn close(&mut self) {
        for s in 0..self.reqs.len() {
            if !self.arrived[s] {
                cancel(&mut self.reqs[s]);
            }
        }
    }
}

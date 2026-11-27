// The sending end of a lane (Sender, in mod.rs): the frames out, the credits in.

extern crate std;

use std::boxed::Box;

use mpi::ffi;

use crate::backends::mpi::memory::layout::{Remote, RouteLayout};
use crate::backends::mpi::transport::cross::calls::{cancel, irecv, isend, null, test, wait};
use crate::backends::mpi::transport::cross::count::Count;
use crate::backends::mpi::types::Answer;

/// The sending end of a lane.
pub(crate) struct Out {
    /// The route's index in the topology (`RouteId`): what the debug dump names a lane by.
    rid: u32,
    comm: ffi::MPI_Comm,
    peer: i32,
    lane: i32,
    size: usize,
    cap: u64,
    lossy: bool,
    /// Frames sent, and the largest credit received.
    p: u64,
    c: u64,
    /// Frame i is sent from buffer i mod cap, by request i mod cap.
    bufs: Box<[u8]>,
    reqs: Box<[ffi::MPI_Request]>,
    credit: Box<u64>,
    credit_req: ffi::MPI_Request,
    /// A lossy frame waiting for room.
    pending: Option<Box<[u8]>>,
    spare: Box<[u8]>,
    /// Frames sent, credits arrived (`Counts`).
    sent: Count,
    got: Count,
}

impl Out {
    /// The end of route `r` that sends, with its credit receive posted.
    pub(super) fn open(rid: usize, r: &RouteLayout, x: Remote, comm: ffi::MPI_Comm, sent: Count, got: Count) -> Out {
        let n = r.size;
        let cap = r.cap as usize;
        let bufs = std::vec![0u8; n * cap].into_boxed_slice();
        let reqs = std::vec![null(); cap].into_boxed_slice();
        let mut o = Out {
            rid: rid as u32,
            comm,
            peer: x.world,
            lane: x.lane,
            size: n,
            cap: r.cap,
            lossy: r.lossy,
            p: 0,
            c: 0,
            bufs,
            reqs,
            credit: Box::new(0),
            credit_req: null(),
            pending: None,
            spare: std::vec![0u8; n].into_boxed_slice(),
            sent,
            got,
        };
        o.post_credit();
        o
    }

    fn post_credit(&mut self) {
        let at = (&mut *self.credit as *mut u64).cast::<u8>();
        irecv(at, 8, self.peer, 2 * self.lane + 1, self.comm, &mut self.credit_req);
    }

    /// Take every credit that has arrived. Answers whether room grew.
    pub(crate) fn poll(&mut self) -> bool {
        let mut grew = false;
        while test(&mut self.credit_req) {
            self.got.add();
            if *self.credit > self.c {
                self.c = *self.credit;
                grew = true;
            }
            self.post_credit();
        }
        if self.p - self.c < self.cap
            && let Some(f) = self.pending.take()
        {
            self.put(&f);
            self.spare = f;
        }
        grew
    }

    fn put(&mut self, msg: &[u8]) {
        let s = (self.p % self.cap) as usize;
        // Frame p - cap left this buffer, and its credit says it was received: the send completes.
        wait(&mut self.reqs[s]);
        let at = &mut self.bufs[s * self.size..(s + 1) * self.size];
        at.copy_from_slice(msg);
        isend(at.as_ptr(), self.size, self.peer, 2 * self.lane, self.comm, &mut self.reqs[s]);
        self.sent.add();
        self.p += 1;
        crate::debug::rec(crate::debug::op::RELAY_OUT, u64::from(self.rid), self.size as u64);
    }

    /// Frames sent, and the largest credit received.
    pub(crate) fn counts(&self) -> (u64, u64) {
        (self.p, self.c)
    }

    /// Send `msg`. Answers Full only on a lossless lane with `cap` frames uncredited; the second
    /// value says whether room grew meanwhile (a credit arrived).
    pub(crate) fn send(&mut self, msg: &[u8]) -> (Answer, bool) {
        let grew = self.poll();
        if self.p - self.c < self.cap {
            self.put(msg);
            (Answer::Ok, grew)
        } else if self.lossy {
            // No allocation: the pending buffer and the spare swap.
            let mut f = self.pending.take().unwrap_or_else(|| core::mem::take(&mut self.spare));
            f.copy_from_slice(msg);
            self.pending = Some(f);
            (Answer::Ok, grew)
        } else {
            (Answer::Full, grew)
        }
    }

    /// Whether a send would find room; the second value as `send`'s.
    pub(crate) fn room(&mut self) -> (bool, bool) {
        let grew = self.poll();
        (self.lossy || self.p - self.c < self.cap, grew)
    }

    /// Complete every send; drop a pending frame. Answers whether all are complete.
    pub(super) fn drain(&mut self) -> bool {
        self.pending = None;
        self.poll();
        self.reqs.iter_mut().fold(true, |all, r| test(r) && all)
    }

    pub(super) fn close(&mut self) {
        cancel(&mut self.credit_req);
    }
}

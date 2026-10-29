// The byte layout of the shared window, and the topology the transport is given. The checks that
// yield a layout are in shape.rs. Every rank computes the same layout from the same topology, so no
// rank ever sends an offset to another. The leader is rank `workers`, and a route may name it as
// either end.
//
// Window layout (all offsets in bytes, every header word on its own 64-byte line):
//
//   GEN                    barrier generations released so far (a counter, not a futex word)
//   COUNT                  barrier arrivals, over all generations
//   PUB_NO                 the publication the workers read: its number (0 is the initial one)
//   PUB_LEN                its length; its bytes are the object `<window>-pub-<PUB_NO>` (object.rs)
//   PEND_SEQ               seqlock of the leader's deposit (u32, futex word; publish.rs). Odd while the
//                          leader writes; the releaser waits on it only while it is odd
//   PEND_NO, PEND_LEN      the deposit: its number and length; its bytes are `<window>-pend-<PEND_NO>`
//   DONE                   bit 0 = aborted; RESULT x workers with a result (u32, futex word)
//   START                  CLOCK_MONOTONIC at launch, written by the leader before the launch barrier
//   LEADER_EPOCH           u32, moves on every act that can change the leader's wait: a frame to the
//                          leader, a worker's result, an abort (epoch.rs)
//   LEADER_SLEEP           1 while the leader sleeps on LEADER_EPOCH
//   RELAY_EPOCH            u32, moves on every act a node's relay must act on (NODES.md); unused on one node
//   RELAY_SLEEP            1 while the relay sleeps on RELAY_EPOCH
//   END                    1 once the leader has joined (NODES.md, End); unused on one node
//   ABORT_WORKER           the rank of the worker whose panic aborted the run (its first abort; the
//                          message is the object `<window>-abort-0`, written before DONE's bit is set)
//   ABORT_LEN              the length of that message
//   ARRIVE                 per worker, five lines each, kind k of worker w at ARRIVE + 64 (k W + w):
//                            k=0 ARRIVE   last barrier generation it arrived at
//                            k=1 EXITED   1 once the worker has returned (barrier, deadlock)
//                            k=2 OUTCOME  its result, 0 until set
//                            k=3 EPOCH    u32, moves on every act that can change its wait conditions
//                            k=4 SLEEP    1 while the worker sleeps on EPOCH (results.rs, epoch.rs)
//   ROUTES                 per route, in rank order:
//                            +0    P: frames ever sent on the route (written by the sender only)
//                            +64   C: frames ever consumed (written by the receiver only; lossless)
//                            +128  cap slots, each: seq (8), payload (8 * words)

extern crate std;

use std::vec::Vec;

pub(crate) const LINE: usize = 64;
pub(crate) const GEN: usize = 0;
pub(crate) const COUNT: usize = LINE;
pub(crate) const PUB_NO: usize = 2 * LINE;
pub(crate) const PUB_LEN: usize = 3 * LINE;
pub(crate) const PEND_SEQ: usize = 4 * LINE;
pub(crate) const PEND_NO: usize = 5 * LINE;
pub(crate) const PEND_LEN: usize = 6 * LINE;
pub(crate) const DONE: usize = PEND_LEN + LINE;
pub(crate) const START: usize = DONE + LINE;
pub(crate) const LEADER_EPOCH: usize = START + LINE;
pub(crate) const LEADER_SLEEP: usize = LEADER_EPOCH + LINE;
pub(crate) const RELAY_EPOCH: usize = LEADER_SLEEP + LINE;
pub(crate) const RELAY_SLEEP: usize = RELAY_EPOCH + LINE;
pub(crate) const END: usize = RELAY_SLEEP + LINE;
pub(crate) const ABORT_WORKER: usize = END + LINE;
pub(crate) const ABORT_LEN: usize = ABORT_WORKER + LINE;
pub(crate) const ARRIVE: usize = ABORT_LEN + LINE;
/// Header lines per worker, one per kind listed above.
pub(crate) const WORKER_LINES: usize = 5;
pub(crate) const K_ARRIVE: usize = 0;
pub(crate) const K_EXITED: usize = 1;
pub(crate) const K_OUTCOME: usize = 2;
pub(crate) const K_EPOCH: usize = 3;
pub(crate) const K_SLEEP: usize = 4;
pub(crate) const SLOT_HEAD: usize = 8;
/// Bit 0 of DONE: some agent panicked, and the run is aborted.
pub(crate) const ABORT_BIT: u32 = 1;
/// Each worker with a result adds this to DONE, so DONE / RESULT counts them.
pub(crate) const RESULT: u32 = 2;
/// Outcome words: what a worker's `run` came to. Zero means no result yet. A panicking worker sets
/// OUT_ABORT only so that its own result is set: the leader panics before it reads any word (results.rs).
pub(crate) const OUT_RETURNED: u64 = 1;
pub(crate) const OUT_ABORT: u64 = 2;
/// The worker was stuck at a barrier and ended with Deadlock (spec.md 5, Deadlock).
pub(crate) const OUT_DEADLOCK: u64 = 3;

/// A route as the host declares it: sender, receiver, tag, message size in bytes, capacity in
/// frames, and whether it is lossy. A worker is named by its rank; the leader is `workers`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub from: u32,
    pub to: u32,
    pub tag: u64,
    pub size: usize,
    pub cap: u32,
    pub lossy: bool,
}

/// The communication graph. Workers are ranks `0..workers`, the leader is rank `workers`; a route's
/// rank is its index in `routes`. `publication` is the initial publication of the one host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Topology {
    pub workers: u32,
    pub routes: Vec<Route>,
    pub publication: Vec<u8>,
}

/// Per-route numbers the run needs, computed once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RouteLayout {
    pub from: u32,
    pub to: u32,
    pub size: usize,
    pub cap: u64,
    pub lossy: bool,
    /// Offset of the route's P word. C is at `base + LINE`, slots start at `base + 2 LINE`.
    pub base: usize,
    /// Bytes per slot: header plus payload rounded up to 8.
    pub stride: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Layout {
    pub workers: u32,
    pub routes: Vec<RouteLayout>,
    pub total: usize,
}

impl Layout {
    /// The word of kind `k` (one of the K_ constants) for worker `w`.
    pub(crate) fn per_worker(&self, k: usize, w: u32) -> usize {
        ARRIVE + LINE * (k * self.workers as usize + w as usize)
    }

    pub(crate) fn slot(&self, r: &RouteLayout, index: u64) -> usize {
        // `index % cap` is below `cap`, which fits in usize because the layout was checked.
        r.base + 2 * LINE + (index % r.cap) as usize * r.stride
    }
}

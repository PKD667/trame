// The byte layout of the shared window, and the topology the transport is given. The checks that
// yield a layout are in shape.rs. Every rank computes the same layout from the same topology, so no
// rank ever sends an offset to another. The leader is rank `workers`, and a route may name it as
// either end.
//
// Window layout (all offsets in bytes, every header word on its own 64-byte line):
//
//   DONE                   bit 0 = aborted; RESULT x workers with a result (u32, futex word)
//   START                  CLOCK_MONOTONIC at launch, written by the leader before the launch barrier
//   LEADER_EPOCH           u32, moves on every act that can change the leader's wait: a frame to the
//                          leader, a worker's result, an abort (epoch.rs)
//   LEADER_SLEEP           1 while the leader sleeps on LEADER_EPOCH
//   ABORT_WORKER           the rank of the worker whose panic aborted the run (its first abort; the
//                          message is the object `<window>-abort-0`, written before DONE's bit is set)
//   ABORT_LEN              the length of that message
//   STOP                   1 once the leader stopped the run (Run::stop); workers poll it
//   DEADLOCK               1 once the leader showed every worker without a result stuck (detect.rs);
//                          each ends with OUT_DEADLOCK at its next round
//   PER_WORKER             per worker, eight lines each, kind k of worker w at PER_WORKER + 64 (k W + w):
//                            k=0 OUTCOME  its result, 0 until set
//                            k=1 EPOCH    u32, moves on every act that can change its wait conditions
//                            k=2 SLEEP    how many of the worker's agents sleep on EPOCH (epoch.rs)
//                            k=3 EDIT     the fields the leader asks the worker to hold for an edit, 0
//                                         if none (written by the leader; leader.rs `edit`)
//                            k=4 HELD     1 while the worker holds them for the leader (written by
//                                         the worker; worker.rs `edit_held`)
//                            k=5 RUN      steps of the worker running now
//                            k=6 ACTS     steps it ever started
//                            k=7 IDLE     EPOCH + 1 when an agent went quiet with no deadline, 0 otherwise
//                                         (5-7 written under the worker's lock by the worker_mark_* hooks,
//                                         read by the leader's detector, detect.rs)
//   ROUTES                 per route, in rank order:
//                            +0    P: frames ever sent on the route (written by the sender only)
//                            +64   C: frames ever consumed (written by the receiver only; lossless)
//                          A route between hosts has no ring: P counts the messages this end of its
//                          lane sent, C those it saw arrive, frames and credits alike (transport/cross/).
//                            +128  cap slots, each: seq (8), payload (8 * words)

extern crate std;

use std::vec::Vec;

pub(crate) const LINE: usize = 64;
pub(crate) const DONE: usize = 0;
pub(crate) const START: usize = DONE + LINE;
pub(crate) const LEADER_EPOCH: usize = START + LINE;
pub(crate) const LEADER_SLEEP: usize = LEADER_EPOCH + LINE;
pub(crate) const ABORT_WORKER: usize = LEADER_SLEEP + LINE;
pub(crate) const ABORT_LEN: usize = ABORT_WORKER + LINE;
pub(crate) const STOP: usize = ABORT_LEN + LINE;
pub(crate) const DEADLOCK: usize = STOP + LINE;
pub(crate) const PER_WORKER: usize = DEADLOCK + LINE;
/// Header lines per worker, one per kind listed above.
pub(crate) const WORKER_LINES: usize = 8;
pub(crate) const K_OUTCOME: usize = 0;
pub(crate) const K_EPOCH: usize = 1;
pub(crate) const K_SLEEP: usize = 2;
pub(crate) const K_EDIT: usize = 3;
pub(crate) const K_HELD: usize = 4;
pub(crate) const K_RUN: usize = 5;
pub(crate) const K_ACTS: usize = 6;
pub(crate) const K_IDLE: usize = 7;
pub(crate) const SLOT_HEAD: usize = 8;
/// Bit 0 of DONE: some agent panicked, and the run is aborted.
pub(crate) const ABORT_BIT: u32 = 1;
/// Each worker with a result adds this to DONE, so DONE / RESULT counts them.
pub(crate) const RESULT: u32 = 2;
/// Outcome words: what a worker's `run` came to. Zero means no result yet. A panicking worker sets
/// OUT_ABORT only so that its own result is set: the leader panics before it reads any word (results.rs).
pub(crate) const OUT_RETURNED: u64 = 1;
pub(crate) const OUT_ABORT: u64 = 2;
/// The worker ended with Deadlock (INTERFACE.md 5, Deadlock): the leader showed it stuck (detect.rs).
pub(crate) const OUT_DEADLOCK: u64 = 3;
/// Every step of the worker ended after a stop (INTERFACE.md, Stop).
pub(crate) const OUT_STOPPED: u64 = 4;
/// A step of the worker failed.
pub(crate) const OUT_FAILED: u64 = 5;

/// A route as the host declares it: sender, receiver, tag, message size in bytes, capacity in
/// frames, and whether it is lossy. A worker is named by its rank; the leader is `workers`. A route
/// to or from a worker of another host has `remote`: then the end on that host is its worker number
/// there, and the route has no ring in the window (transport/cross/).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub from: u32,
    pub to: u32,
    pub tag: u64,
    pub size: usize,
    pub cap: u32,
    pub lossy: bool,
    pub remote: Option<Remote>,
}

/// The end of a route on another host (transport/cross/).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Remote {
    /// That end's host.
    pub host: u32,
    /// Whether that end is the receiver: the route leaves this host.
    pub outgoing: bool,
    /// That end's rank in the world.
    pub world: i32,
    /// The route's frames travel with MPI tag `2 lane`, its credits with `2 lane + 1` (transport/cross/).
    pub lane: i32,
}

/// The communication graph. Workers are ranks `0..workers`, the leader is rank `workers`; a route's
/// rank is its index in `routes`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Topology {
    pub workers: u32,
    pub routes: Vec<Route>,
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
    pub remote: Option<Remote>,
}

impl RouteLayout {
    /// Whether the endpoint of rank `me` sends on this route. A route from another host is sent there.
    pub(crate) fn sent_by(&self, me: u32) -> bool {
        self.from == me && self.remote.is_none_or(|x| x.outgoing)
    }

    /// Whether the endpoint of rank `me` receives this route.
    pub(crate) fn received_by(&self, me: u32) -> bool {
        self.to == me && self.remote.is_none_or(|x| !x.outgoing)
    }
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
        PER_WORKER + LINE * (k * self.workers as usize + w as usize)
    }

    pub(crate) fn slot(&self, r: &RouteLayout, index: u64) -> usize {
        // `index % cap` is below `cap`, which fits in usize because the layout was checked.
        r.base + 2 * LINE + (index % r.cap) as usize * r.stride
    }
}

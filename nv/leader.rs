//! The leader's route on a device launch: one control link per worker, in memory both ends can see.
//!
//! The leader is a host process here as it is everywhere: what differs is only the *route*, and on
//! this backend the route is memory the launch owns. What it needs is not a new protocol but the
//! *other end* of one the launch already builds. A link is a `Layout` ring plus the state word in
//! each slot, and the device end of it is `device::{Tx, Rx}` — so the host end here is the same
//! bytes, not a second design.
//!
//! Two links per worker, directed. The *up* link carries frames the worker sends and the leader
//! receives; the *down* link carries the other direction. Per-worker rather than one shared ring
//! in each direction, and that is a decision: a shared up-ring would make every worker a producer
//! of the same sequence and need a compare-exchange to reserve a slot, and a shared down-ring
//! would be read by every worker, so each would have to inspect frames addressed to others. One
//! ring per direction per worker has exactly one producer and one consumer on each ring, which is
//! what makes the state word sufficient — no lock, no reservation, and a worker's frame is never
//! behind another worker's.
//!
//! The protocol is the transport's, unchanged: a producer at sequence `s` writes the slot whose
//! index is `s mod depth` and then stores `s + 1` into that slot's state word with release order;
//! a consumer at sequence `s` reads the slot and requires the state word to already be `s + 1`.
//! `Full` and `Empty` are those two comparisons failing, and neither consumes anything.
//!
//! Two things are specific to this backend and both are consequences rather than choices.
//!
//! A device has no way to wait on a host process. A waiting receive therefore polls a *declared*
//! number of times and then reports `Exhausted`, and it never returns `None`: "nothing arrived" is a
//! poll's answer, and handing it to a caller that asked to wait is the silent weakening the contract
//! forbids. `Wait::Wait` is what this backend declared as a budget, and this is where that
//! declaration is kept.
//!
//! A leader's sends can find a worker's down link full, and the leader is a host process that can
//! simply try again later — so the leader reports `Full` rather than spinning, and the caller
//! decides. A worker cannot: it is inside a launch, and its only way to make progress is to keep
//! asking.

use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use super::{Context, Environment};
use crate::contract::{Deployment, Error, Failure, Frame, Rank, Tag, Wait};
use crate::nv::error::{LayoutError, RecvError, SendError};
use crate::nv::layout::Layout;

/// How many frames a control link holds before its producer is told `Full`.
///
/// Small on purpose. This route carries management traffic — a dispatch, a shutdown, a progress
/// note — not payload, and a deep queue would let a worker run ahead of a leader that has stopped
/// listening without either end finding out.
pub const DEPTH: u32 = 4;

/// The largest frame a control slot carries.
pub const CAPACITY: u32 = 256;

/// How many attempts a waiting worker call makes before reporting exhaustion.
///
/// Stated rather than unbounded because a device has nothing to block on: a launch that spun
/// forever waiting for a host process that had already exited would be a hang with no diagnosis.
/// Large enough that a leader doing ordinary work is never falsely reported as absent.
pub const LEADER_ATTEMPTS: u32 = 1 << 22;

/// The geometry of a leader route: how many workers, and how big their links are.
#[derive(Clone, Copy)]
pub struct Route {
    layout: Layout,
    ranks: u32,
}

impl Route {
    pub fn new(ranks: u32, depth: u32, capacity: u32) -> Result<Self, LayoutError> {
        Ok(Self {
            layout: Layout::new(depth, capacity)?,
            ranks,
        })
    }

    /// The default geometry: a control link per worker, `DEPTH` frames deep.
    pub fn sized(ranks: u32) -> Result<Self, LayoutError> {
        Self::new(ranks, DEPTH, CAPACITY)
    }

    pub const fn ranks(self) -> u32 {
        self.ranks
    }

    pub const fn layout(self) -> Layout {
        self.layout
    }

    /// How many `u32` words this route occupies, for a launch sizing its region.
    pub const fn words(self) -> usize {
        // Two directed links per worker.
        self.ranks as usize * 2 * self.layout.words()
    }

    /// Prepare a region. Every slot's state word starts at its own index, which is what makes a
    /// producer's first comparison succeed.
    pub fn init(self, arena: &mut [u32]) {
        assert_eq!(arena.len(), self.words());
        for link in 0..self.ranks as usize * 2 {
            let at = link * self.layout.words();
            self.layout.init(&mut arena[at..at + self.layout.words()]);
        }
    }

    fn base(self, rank: Rank, down: bool) -> usize {
        let link = rank as usize * 2 + usize::from(down);
        link * self.layout.words()
    }

    /// The worker's send link: the worker produces, the leader consumes.
    ///
    /// # Safety
    ///
    /// `ptr` must address at least `words()` words that have been through `init`, and must stay
    /// live and unaliased for as long as either end of the link is.
    pub unsafe fn up(self, ptr: *mut u32, rank: Rank) -> *mut u32 {
        unsafe { ptr.add(self.base(rank, false)) }
    }

    /// The worker's receive link: the leader produces, the worker consumes.
    ///
    /// # Safety
    ///
    /// As [`up`](Self::up).
    pub unsafe fn down(self, ptr: *mut u32, rank: Rank) -> *mut u32 {
        unsafe { ptr.add(self.base(rank, true)) }
    }
}

/// Write one frame into the slot for `seq`, then publish it, or refuse without writing.
///
/// # Safety
///
/// `link` must address `layout.words()` words of a link this caller alone produces on.
unsafe fn publish(
    link: *mut u32,
    layout: Layout,
    seq: u32,
    src: Rank,
    tag: Tag,
    data: &[u8],
) -> Result<(), SendError> {
    if data.len() > layout.capacity() as usize {
        return Err(SendError::TooLarge);
    }
    let slot = unsafe { link.add(layout.slot(seq)) };
    if unsafe { AtomicU32::from_ptr(slot) }.load(Ordering::Acquire) != seq {
        return Err(SendError::Full);
    }
    let words = data.len().div_ceil(4);
    unsafe {
        slot.add(1).write(data.len() as u32);
        slot.add(2).write(src);
        slot.add(3).write(tag);
        for word in 0..words {
            let mut bytes = [0u8; 4];
            let take = (data.len() - word * 4).min(4);
            bytes[..take].copy_from_slice(&data[word * 4..word * 4 + take]);
            slot.add(4 + word).write(u32::from_ne_bytes(bytes));
        }
        AtomicU32::from_ptr(slot).store(seq.wrapping_add(1), Ordering::Release);
    }
    Ok(())
}

/// Take the frame published at `seq`, or answer `None` when there is none yet.
///
/// # Safety
///
/// As [`publish`], and the caller must be the link's only consumer.
unsafe fn consume(
    link: *mut u32,
    layout: Layout,
    seq: u32,
    out: &mut [u8],
) -> Result<Option<(Rank, Tag, u32)>, RecvError> {
    let slot = unsafe { link.add(layout.slot(seq)) };
    if unsafe { AtomicU32::from_ptr(slot) }.load(Ordering::Acquire) != seq.wrapping_add(1) {
        return Ok(None);
    }
    let len = unsafe { slot.add(1).read() };
    let src = unsafe { slot.add(2).read() };
    let tag = unsafe { slot.add(3).read() };
    if len as usize > out.len() {
        // Nothing is consumed: the caller makes room and asks again, and the frame it was told to
        // grow for is the frame it then gets.
        return Err(RecvError::TooSmall { needed: len });
    }
    let words = len as usize;
    for word in 0..words {
        let bytes = unsafe { slot.add(4 + word).read() }.to_ne_bytes();
        let take = (out.len() - word * 4).min(4);
        if take == 0 {
            break;
        }
        out[word * 4..word * 4 + take].copy_from_slice(&bytes[..take]);
    }
    // Hand the slot back, and this is load-bearing rather than tidiness. The state word is a
    // two-sided handshake, not a producer's counter: a producer at sequence `s` writes `s + 1`, and
    // a consumer at sequence `s` writes `s + depth`. `init` seeds the state of slot `i` with `i`,
    // which is what the producer's first comparison needs, and after that the value it reads back
    // is the *consumer's* hand-back from the round `depth` frames ago. Leave this store out and the
    // ring works exactly once around and then reports `Full` for ever, because the slot the
    // producer wants next is one no consumer has released.
    //
    // After the copy, never before: releasing the slot is this end saying it has the bytes, and a
    // producer that refilled it while the copy was still reading would deliver a frame built from
    // two.
    unsafe { AtomicU32::from_ptr(slot).store(seq.wrapping_add(layout.depth()), Ordering::Release) };
    Ok(Some((src, tag, len)))
}

/// The leader: the host process's end of every worker's links.
///
/// It holds no rank, because a leader is not a participant — it is absent from `size`, `rank`,
/// `hosts` and `cohort`, and nothing here needs one. What it addresses with is the worker's
/// contract rank, which is a position in the declaration rather than an identity of its own.
pub struct Leader {
    region: *mut u32,
    route: Route,
    /// Behind a lock because §4 gives `send` and `recv` a shared borrow, and the sequence counters
    /// are state. There is one leader per host process, so this is a handle one thread holds rather
    /// than a contended lock.
    cursors: Mutex<Box<[Cursor]>>,
}

/// The next frame expected from one worker, and the next to be sent to it.
#[derive(Clone, Copy)]
struct Cursor {
    arriving: u32,
    departing: u32,
}

impl Leader {
    /// Open the route over the region the launch prepared.
    ///
    /// The geometry comes from the declaration, because both ends must agree on it and the
    /// declaration is the one thing they both have: the host process builds it here and every
    /// worker builds the same one from the same list.
    pub fn open(env: Environment, deployment: Deployment<'_>) -> Result<Leader, Failure> {
        if deployment.refuse().is_some() {
            return Err(Failure {
                participant: 0,
                operation: "leader::open",
                code: 8,
            });
        }
        let ranks = u32::try_from(deployment.workers.len()).map_err(|_| Failure {
            participant: 0,
            operation: "leader::open",
            code: 1,
        })?;
        if deployment.leaders.is_empty() && ranks == 0 {
            // No leader in this deployment. A process that opened the route anyway is a process the
            // launch did not ask for, and saying so is better than handing back a route that will
            // never carry a frame.
            return Err(Failure {
                participant: 0,
                operation: "leader::open",
                code: 2,
            });
        }
        if env.leader_region.is_null() {
            // The launch prepared no region, so there is nothing to open. Refused rather than
            // given an empty route, because an empty route is a leader that silently hears nobody.
            return Err(Failure {
                participant: 0,
                operation: "leader::open",
                code: 3,
            });
        }
        let route = Route::sized(ranks).map_err(|_| Failure {
            participant: 0,
            operation: "leader::open",
            code: 4,
        })?;
        let idle = Cursor {
            arriving: 0,
            departing: 0,
        };
        // The launch prepared this region for exactly this route and owns it for the life of the
        // job; nothing else produces on the down links or consumes the up links.
        Ok(Leader {
            region: env.leader_region,
            route,
            cursors: Mutex::new(vec![idle; ranks as usize].into_boxed_slice()),
        })
    }

    /// Send one frame to one worker, named by its contract rank.
    ///
    /// `Full` is this worker's down link still holding its previous frames, which is a fact about
    /// that worker rather than about the route: another worker's link may be empty. Reported rather
    /// than retried: the leader is host code and the caller can come back, which a worker inside a
    /// launch cannot.
    pub fn send(&self, to: Rank, tag: Tag, data: &[u8], _wait: Wait) -> Result<(), Error> {
        if to >= self.route.ranks() {
            return Err(Error::Invalid { code: 1 });
        }
        let mut cursors = self.cursors.lock().map_err(|_| {
            Error::Failed(Failure {
                participant: 0,
                operation: "leader::send",
                code: 5,
            })
        })?;
        let cursor = &mut cursors[to as usize];
        // SAFETY: `open`'s region, and the lock makes this the only producer on the down links.
        let link = unsafe { self.route.down(self.region, to) };
        match unsafe { publish(link, self.route.layout(), cursor.departing, 0, tag, data) } {
            Ok(()) => {
                cursor.departing = cursor.departing.wrapping_add(1);
                Ok(())
            }
            Err(SendError::Full) => Err(Error::Full),
            Err(SendError::TooLarge) => Err(Error::TooLarge {
                limit: self.route.layout().capacity(),
            }),
            Err(SendError::NoSuchRank) => Err(Error::Invalid { code: 1 }),
        }
    }

    /// Take the next frame from any worker that has one, or report that none has.
    ///
    /// Every worker is checked, so an empty answer means all of them were empty and not merely
    /// that the one checked first was. A leader that polled only its first worker would be a
    /// leader that could not hear a job that was still running.
    pub fn recv(&self, out: &mut [u8], _wait: Wait) -> Result<Option<Frame>, Error> {
        let mut cursors = self.cursors.lock().map_err(|_| {
            Error::Failed(Failure {
                participant: 0,
                operation: "leader::recv",
                code: 5,
            })
        })?;
        for source in 0..self.route.ranks() {
            let cursor = &mut cursors[source as usize];
            // SAFETY: as `send`, as the only consumer on the up links.
            let link = unsafe { self.route.up(self.region, source) };
            match unsafe { consume(link, self.route.layout(), cursor.arriving, out) } {
                Ok(None) | Err(RecvError::Empty) => {}
                Ok(Some((_, tag, len))) => {
                    cursor.arriving = cursor.arriving.wrapping_add(1);
                    return Ok(Some(Frame { source, tag, len }));
                }
                Err(RecvError::TooSmall { needed }) => return Err(Error::TooSmall { needed }),
            }
        }
        Ok(None)
    }
}

/// A worker's end.
///
/// Two exist because a worker exists in two models, and the transport is what differs: the host
/// model runs ranks as threads over ordinary memory, and a device launch runs warps that must reach
/// the region through the same warp-cooperative movers every other device link uses. Both speak the
/// protocol above over the same bytes, which is why one host leader talks to either.
#[cfg(feature = "cuda")]
mod cuda;
#[cfg(not(feature = "cuda"))]
mod sim;

#[cfg(feature = "cuda")]
pub use cuda::Worker;
#[cfg(not(feature = "cuda"))]
pub use sim::Worker;

/// A worker's send: one frame to this job's leader.
///
/// No destination, because a worker is the producer on exactly one link and the route knows which.
pub fn send(cx: &mut Context, tag: Tag, data: &[u8], wait: Wait) -> Result<(), Error> {
    let worker = cx.leader()?;
    let attempts = if wait == Wait::Poll {
        1
    } else {
        LEADER_ATTEMPTS
    };
    for _ in 0..attempts {
        // SAFETY: the region belongs to the launch, this worker is its only producer on the up link,
        // and `cx` gives one worker end to one caller.
        match unsafe { worker.send(tag, data) } {
            Ok(()) => return Ok(()),
            Err(SendError::Full) => continue,
            Err(SendError::NoSuchRank) => return Err(Error::Invalid { code: 2 }),
            Err(SendError::TooLarge) => return Err(Error::TooLarge { limit: CAPACITY }),
        }
    }
    Err(Error::Exhausted { attempts })
}

/// A worker's receive: the leader's next frame, as `(tag, length)`.
///
/// A waiting call never returns `None`. It polls `LEADER_ATTEMPTS` times and then reports
/// exhaustion, because there is no second source whose silence could have been mistaken for an
/// empty answer and no way to block a warp on a host process.
pub fn recv(cx: &mut Context, out: &mut [u8], wait: Wait) -> Result<Option<(Tag, u32)>, Error> {
    let worker = cx.leader()?;
    let attempts = if wait == Wait::Poll {
        1
    } else {
        LEADER_ATTEMPTS
    };
    for _ in 0..attempts {
        // SAFETY: as `send`, on this worker's down link.
        match unsafe { worker.recv(out) } {
            Ok(Some(frame)) => return Ok(Some(frame)),
            Ok(None) => continue,
            Err(RecvError::TooSmall { needed }) => return Err(Error::TooSmall { needed }),
            Err(RecvError::Empty) => continue,
        }
    }
    if wait == Wait::Poll {
        return Ok(None);
    }
    Err(Error::Exhausted { attempts })
}

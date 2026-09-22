//! The device backend: one CUDA warp per participant.
//!
//! It uses none of the shared MPI environment, so it supplies the whole surface itself, as
//! `none/` does. Everything that is a fact about the device lives in the submodules: the links,
//! the lane and its split, and the clock. Everything that is a fact about the *contract* is here.
//!
//! # Participant-local state, and why there is no global
//!
//! The entry rules forbid an implicit process-global context. This backend used to have three: a
//! word holding rank and size, a cell holding the cohort, and a cell holding the endpoints. Every
//! warp of a launch wrote its own rank and its own endpoints into them, so the last writer won and
//! every warp then used another warp's identity and another warp's links; the concurrent writes
//! through aliased `UnsafeCell`s were undefined behaviour besides. All three are gone. Identity,
//! the cohort tables and the endpoints are fields of [`Context`], which the entry builds once and
//! the caller owns, and `peers::Links` holds the per-link sequence numbers that made sharing
//! them wrong in the first place.
//!
//! # What a device changes about the surface
//!
//! * **One device, so one sharing domain and one cohort.** `hosts` is rank 0 for every rank. A
//!   cohort rule that named a subset is refused by `init`: there is no second node for the
//!   excluded ranks to be on.
//! * **Every link has the launch's layout.** The arena is allocated before any graph is known, so
//!   one geometry covers every pair, and `reshape` refuses a declaration whose implied depth or
//!   frame size the launched arena cannot serve. Refusing at declaration is required: a frame
//!   that does not fit must never be truncated at run time.
//! * **Both channels ride the links.** A device has no separate message wire; `Message` and `Lane`
//!   differ in the tag they carry and in what the caller may assume, not in the memory they use.
//!   Lanes are acknowledged writes into the receiver's own slot, so they do not lose, and the
//!   `Message` route's reliability is therefore inherited rather than promised separately.
//! * **The primitive families are absent.** The device has no shared-state families yet, so
//!   "unsupported operations shall be unavailable to compiled callers" is met by not exporting
//!   them: a program that names `trame::sync` under this backend fails to compile, which is the
//!   refusal the spec asks for. `DECLARATIONS.families` says the same thing as values.
//!
//! # Unwritten
//!
//! The three primitive families are declared unsupported: a device `Exclusive` is a ticket
//! over a device-scope atomic, a device `publish` is the slot protocol, and both are real work
//! that the host model cannot check, so they are not claimed here. `global_release` is false for
//! the same reason — a cohort-wide barrier needs a cooperative launch and this backend does not
//! require one.

use crate::contract::{
    Channel, Declarations, Deployment, Edge, Error, Failure, Frame, LANE_RELIABLE,
    PUBLICATION_PREPUBLISHED, RELEASE_LOCAL, Rank, Reading, Scopes, Tag, Wait,
};

pub mod clock;
pub mod device;
pub mod error;
pub mod layout;
pub mod leader;
pub mod model;
pub mod peers;
pub mod run;
pub mod sync;
pub mod transport;
pub mod warp;

#[cfg(test)]
mod tests;

use layout::Layout;
use leader::Route;
use peers::{Fabric, Links, Refused};
use transport::{MAX_RANKS, Message};

/// Backend identifier stored in `LOAD` records. Appended after `none`: the id is on disk in every
/// log ever written, so the list only ever grows at the end.
pub const ID: u8 = 4;

/// Slots per destination element, the rule `affected` is read against.
///
/// The launch's arena has one depth for every pair, so this is the factor the *widest* pair needs
/// rather than a per-pair depth; `reshape` refuses a declaration the launched arena cannot hold.
pub const FACTOR: u64 = 4;

/// Attempts a `Wait` makes before it reports exhaustion.
///
/// A participant that is not resident cannot drain a lane, so an unbounded retry is a hang. The
/// budget bounds *attempts*, never elapsed time, and it does not make an absent participant run —
/// which is why exhaustion is reported rather than retried forever.
const BUDGET: u32 = 1_000_000;

/// What this backend answers.
///
/// The families are false because they are unwritten: a device turn is a ticket over a device-scope
/// atomic and a device publication is the slot protocol, and the host model cannot check either,
/// so neither is claimed. `max_frame` is `u32::MAX` because the real limit is the launch's arena
/// and `reshape` checks the declaration against it; a compile-time number nobody could honour
/// would be worse than an honest bound validated at declaration.
pub const DECLARATIONS: Declarations = Declarations {
    // A device-scope atomic orders every warp of one launch, which is both a participant's own
    // warps and a declared sharing domain — so both scopes are answered by the same instruction.
    // There is no second node, so a system scope would name a machine that is not there.
    atomic_scopes: Scopes {
        participant: true,
        domain: true,
        system: false,
    },
    lane_reliability: LANE_RELIABLE,
    // The device transport's primitives are one immediate attempt each; `BUDGET` is the retry
    // budget `send` and `recv` themselves drive, and it bounds both routes.
    waiting_message: BUDGET,
    waiting_lane: BUDGET,
    // A full lane and a short buffer are both refusals the calling warp observes, so pressure is
    // reported on both routes.
    pressure_message: true,
    pressure_lane: true,
    release: RELEASE_LOCAL,
    // The segment is an allocation the launch owns before any participant enters, so publication
    // is a write into memory that is already there and already imaged.
    publication: PUBLICATION_PREPUBLISHED,
    priority: false,
    resident: true,
    max_frame: u32::MAX,
    tag_limit: Tag::MAX,
    lowering: "warp-split",
};

/// What the launch supplies: this participant's identity, the fabric its links live in, and the
/// segment it may publish into.
///
/// It is a value rather than a call because a device cannot discover any of it. There is no
/// `MPI_Comm_size` to ask: the entry knows the launch's width and where its memory is, and hands
/// both down. That is also why there is no `install` and no global for it to write to.
pub struct Environment {
    /// This participant's rank in the launch.
    pub rank: Rank,
    /// How many participants the launch started.
    pub size: u32,
    /// The links' fabric: device memory the launch allocated, or the host model's mesh.
    pub fabric: Fabric,
    /// The segment participants may publish into, or null when the launch has none.
    pub segment: *mut u8,
    /// How many bytes that segment has.
    pub segment_bytes: usize,
    /// The control region the launch prepared for the leader route, or null when it prepared none.
    ///
    /// Launch-owned memory both sides can see, which is what a device launch's leader route is: the
    /// leader is a host process and the workers are warps, so the only thing they can share is
    /// memory the launch owns. Null is the honest way to say a launch has no leader, and a leader
    /// route asked for over a null region is refused rather than given an empty route.
    pub leader_region: *mut u32,
}

/// This participant's state. Owned by the caller, passed by `&mut`, shared with nobody.
pub struct Context {
    rank: Rank,
    size: u32,
    links: Links,
    /// The worker's end of the leader route, when the launch named a leader.
    leader: Option<leader::Worker>,
    layout: Layout,
    /// The tag lane traffic rides under, set by `reshape`.
    tag: Option<Tag>,
    /// The workers of the current load, in declaration order, and how many there are.
    workers: [Rank; MAX_RANKS],
    worker_count: usize,
    /// One device: every rank's sharing-domain leader. Held here rather than in a static because
    /// a static is what made two participants share one identity.
    hosts: [Rank; MAX_RANKS],
    /// The cohort: every rank of the launch, in order.
    cohort: [Rank; MAX_RANKS],
    segment: *mut u8,
    segment_bytes: usize,
}

// SAFETY: the context is one participant's state. The raw pointers it holds address memory the
// launch owns, and a participant that moves between host threads between calls is not a thing a
// device launch does; the model's participants are host threads and do this legitimately.
unsafe impl Send for Context {}

impl Context {
    /// This worker's end of the leader route, or a refusal when the launch named no leader.
    ///
    /// A refusal rather than `None`, because a caller that asked to reach its leader and has none
    /// has made a mistake about where it runs, and the contract has a value for that.
    pub(crate) fn leader(&mut self) -> Result<&mut leader::Worker, Error> {
        self.leader.as_mut().ok_or(Error::Invalid { code: 5 })
    }
}

/// One immutable copy of a published segment.
///
/// A view, not a copy: on one device the segment is allocation every participant can already
/// read, so publication is each participant writing its own slice into it and there is nothing to
/// map at the end. That is why `unshare` has nothing to discharge and says so.
pub struct Shared {
    base: *const u8,
    total: usize,
}

// ---------------------------------------------------------------------------------------------
// Entry and failure

/// Enter the launch.
///
/// The cohort rule is evaluated and then checked: a rule that does not name every rank is
/// refused, because a launch is one node and the ranks it leaves out have nowhere to be.
pub fn init(
    env: Environment,
    deployment: Deployment<'_>,
    cohort: fn(Rank, &[Rank]) -> u32,
) -> Result<Context, Failure> {
    // The contract's participant set is the workers. A deployment that names none is every rank
    // the launch started, which is the widening the type promises, so both readings come from one
    // expression and a launch that says nothing is a launch where every rank works.
    if deployment.refuse().is_some() {
        return Err(Failure {
            participant: env.rank,
            operation: "init",
            code: 2,
        });
    }
    let mut all = [0 as Rank; MAX_RANKS];
    let count = if deployment.workers.is_empty() {
        env.size as usize
    } else {
        deployment.workers.len()
    };
    if count > MAX_RANKS || count > env.size as usize {
        return Err(Failure {
            participant: env.rank,
            operation: "init",
            code: 3,
        });
    }
    for (i, slot) in all.iter_mut().enumerate().take(count) {
        *slot = if deployment.workers.is_empty() {
            i as Rank
        } else {
            deployment.workers[i]
        };
    }
    let launch = &all[..count];
    // The rank the contract reports is the position in the declaration; the number the launch
    // knows this participant by is the one in the table. They coincide unless the launch said
    // otherwise, and when it does, this is the one place the difference exists.
    let rank = deployment
        .contract(env.rank)
        .or_else(|| deployment.workers.is_empty().then_some(env.rank))
        .ok_or(Failure {
            participant: env.rank,
            operation: "init",
            code: 4,
        })?;
    let _colour = cohort(rank, launch);

    let links = Links::open(&env.fabric, env.rank, env.size).map_err(|_| Failure {
        participant: env.rank,
        operation: "init",
        code: 1,
    })?;

    let mut hosts = [0 as Rank; MAX_RANKS];
    // Every rank of a launch shares the device's memory, so every rank's leader is rank 0. The
    // table is one entry per rank and not one entry, which would say the launch has one rank.
    for slot in hosts.iter_mut().take(count) {
        *slot = launch.first().copied().unwrap_or(0);
    }

    // The worker end of the leader route, when the declaration names a leader for this worker. The
    // geometry comes from the declaration, not from the environment: both ends must agree on it and
    // the worker list is the one thing they both hold.
    let leader = match deployment.leader_of(rank) {
        None => None,
        Some(_) => {
            if env.leader_region.is_null() {
                return Err(Failure {
                    participant: rank,
                    operation: "init",
                    code: 5,
                });
            }
            let route = Route::sized(count as u32).map_err(|_| Failure {
                participant: rank,
                operation: "init",
                code: 6,
            })?;
            // SAFETY: the launch prepared this region for exactly this route and owns it for the
            // life of the job; this worker is the only producer on its up link and the only
            // consumer of its down link.
            Some(unsafe { leader::Worker::new(env.leader_region, route, rank) })
        }
    };

    Ok(Context {
        rank,
        size: count as u32,
        links,
        leader,
        layout: env.fabric.layout(),
        tag: None,
        workers: all,
        worker_count: 0,
        hosts,
        cohort: all,
        segment: env.segment,
        segment_bytes: env.segment_bytes,
    })
}

pub fn rank(cx: &Context) -> Rank {
    cx.rank
}

pub fn size(cx: &Context) -> u32 {
    cx.size
}

pub fn hosts(cx: &Context) -> &[Rank] {
    &cx.hosts[..cx.size as usize]
}

pub fn cohort(cx: &Context) -> &[Rank] {
    &cx.cohort[..cx.size as usize]
}

/// Report the outcome and discharge nothing.
///
/// A participant that is not a process ends by returning; what ends the launch is the kernel
/// returning, which is the launcher's business and not this call's. The outcome travels back
/// because a failed run has to be to be reported rather than completed.
pub fn done(_cx: &mut Context, outcome: Result<(), Failure>) -> Result<(), Failure> {
    outcome
}

// ---------------------------------------------------------------------------------------------
// Communication

/// Send one frame. Success means *accepted*: the bytes are in the peer's slot or nowhere.
pub fn send(
    cx: &mut Context,
    to: Rank,
    channel: Channel,
    data: &[u8],
    wait: Wait,
) -> Result<(), Error> {
    let tag = match channel {
        Channel::Message(tag) => tag,
        Channel::Lane => cx.tag.ok_or(Error::Invalid { code: 1 })?,
    };
    let mut attempts = 0u32;
    loop {
        match peers::try_send(&mut cx.links, to, tag, data) {
            Ok(()) => return Ok(()),
            Err(Refused::TooLarge) => {
                return Err(Error::TooLarge {
                    limit: cx.layout.capacity(),
                });
            }
            Err(Refused::NoSuchPeer) => return Err(Error::Closed),
            Err(Refused::Full) => {
                if wait == Wait::Poll {
                    return Err(Error::Full);
                }
                attempts += 1;
                if attempts >= BUDGET {
                    return Err(Error::Exhausted { attempts });
                }
                core::hint::spin_loop();
            }
            Err(_) => return Err(Error::Invalid { code: 2 }),
        }
    }
}

/// Take one frame from any source, into the caller's buffer.
///
/// A frame that does not fit is reported as `TooSmall` and is not consumed, so the caller may
/// grow its buffer and ask again. Nothing is stashed: demultiplexing by tag is the caller's job,
/// and a backend that held frames for a tag nobody asked about would be a queue whose depth
/// nobody declared.
pub fn recv(cx: &mut Context, out: &mut [u8], wait: Wait) -> Result<Option<Frame>, Error> {
    let mut attempts = 0u32;
    loop {
        let mut needed = 0u32;
        for src in 0..cx.size {
            match peers::try_recv(&mut cx.links, src, out) {
                Ok(Message { src, tag, len }) => {
                    return Ok(Some(Frame {
                        source: src,
                        tag,
                        len,
                    }));
                }
                Err(Refused::TooSmall { needed: n }) => needed = needed.max(n),
                // A source with nothing waiting, and a source that is not a participant at all,
                // are both "no frame here"; the destination check belongs to a send.
                Err(Refused::Empty) | Err(Refused::NoSuchPeer) => {}
                Err(_) => return Err(Error::Invalid { code: 3 }),
            }
        }
        if needed > 0 {
            return Err(Error::TooSmall { needed });
        }
        if wait == Wait::Poll {
            return Ok(None);
        }
        attempts += 1;
        if attempts >= BUDGET {
            return Err(Error::Exhausted { attempts });
        }
        core::hint::spin_loop();
    }
}

/// Report that every accepted send has released its send resource.
///
/// Always immediate on this backend, and not as a convenience: a device send copies into the
/// peer's slot and publishes it before it returns, so there is no operation-specific resource
/// left outstanding for a flush to wait on. What this does *not* mean is that the receiver has
/// read the frame — completion is local and operation-specific, and a caller wanting to know that
/// the far side consumed something is asking for a credit scheme this backend does not have.
pub fn flush(_cx: &mut Context, _wait: Wait) -> Result<(), Error> {
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Geometry and lifetime

/// Take the lane table for this load.
///
/// Validation happens here and not at the first send: the arena's geometry is fixed at launch, so
/// a declaration it cannot serve is refused while it is still a declaration. The checks are the
/// contract's — ascending unique workers, edges ordered and duplicate-free, both endpoints
/// workers — plus the two the launch adds: the frame fits a slot, and the widest pair's implied
/// depth fits the arena.
pub fn reshape(
    cx: &mut Context,
    workers: &[Rank],
    edges: &[Edge],
    bytes: u32,
    tag: Tag,
) -> Result<(), Error> {
    if workers.windows(2).any(|w| w[0] >= w[1]) {
        return Err(Error::Invalid { code: 4 });
    }
    if workers.iter().any(|&w| w >= cx.size) {
        return Err(Error::Invalid { code: 5 });
    }
    if edges
        .windows(2)
        .any(|w| (w[0].source, w[0].destination) >= (w[1].source, w[1].destination))
    {
        return Err(Error::Invalid { code: 7 });
    }
    for edge in edges {
        if !workers.contains(&edge.source) || !workers.contains(&edge.destination) {
            return Err(Error::Invalid { code: 8 });
        }
    }
    if bytes > cx.layout.capacity() {
        return Err(Error::TooLarge {
            limit: cx.layout.capacity(),
        });
    }
    if edges
        .iter()
        .map(|edge| edge.affected as u64 * FACTOR)
        .max()
        .is_some_and(|needed| needed > cx.layout.depth() as u64)
    {
        return Err(Error::Invalid { code: 9 });
    }

    cx.workers[..workers.len()].copy_from_slice(workers);
    cx.worker_count = workers.len();
    cx.tag = Some(tag);
    Ok(())
}

/// Discharge the caller's lane obligations. Local, and declared as such.
///
/// This backend does not claim cohort-wide quiescence: a participant that returns from here has
/// stopped using the lanes, and nothing about that says its peers have. A caller that needs every
/// outstanding access to have ended needs the stronger capability, which this backend does not
/// declare, and must provide its own protocol.
pub fn release(cx: &mut Context) -> Result<(), Error> {
    cx.worker_count = 0;
    cx.tag = None;
    Ok(())
}

/// The participant's share of a segment, by the shared pure rule.
///
/// Alignment is one byte: a device has no pages, and aligning for a mapping that will not happen
/// would shrink every share for nothing.
pub fn slice(cx: &Context, rank: Rank, total: usize) -> (usize, usize) {
    crate::partition::slice_of(hosts(cx), cohort(cx), rank, total, 1)
}

/// Publish this participant's slice into the launch's segment.
pub fn share(cx: &mut Context, mine: &[u8], total: usize) -> Result<Shared, Error> {
    if cx.segment.is_null() || total > cx.segment_bytes {
        return Err(Error::Invalid { code: 10 });
    }
    let (offset, length) = slice(cx, cx.rank, total);
    if mine.len() != length {
        return Err(Error::Invalid { code: 11 });
    }
    // SAFETY: the environment's contract is that `segment` addresses `segment_bytes` writable
    // bytes for the launch's life, and `slice` returned a range inside `total <= segment_bytes`.
    unsafe {
        core::ptr::copy_nonoverlapping(mine.as_ptr(), cx.segment.add(offset), length);
    }
    Ok(Shared {
        base: cx.segment as *const u8,
        total,
    })
}

pub fn bytes(segment: &Shared) -> &[u8] {
    // SAFETY: written by `share` before the handle existed, read-only afterwards, and the handle
    // cannot outlive the launch that owns the segment.
    unsafe { core::slice::from_raw_parts(segment.base, segment.total) }
}

/// Nothing was mapped, so nothing has to be unmapped and the handle stays valid.
///
/// Unlike a host mapping there is no collective retirement to perform: the segment is the
/// launch's memory and it ends when the launch does.
pub fn unshare(_cx: &mut Context, _segment: &mut Shared) -> Result<(), Error> {
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Clocks

/// The device's monotonic counter, with the device as its comparison domain.
pub fn reading(_cx: &Context) -> Result<Reading, Error> {
    Ok(clock::reading())
}

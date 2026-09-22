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
//!   refusal the spec asks for.
//!
//! # Unwritten
//!
//! The three primitive families are declared unsupported: a device `Exclusive` is a ticket
//! over a device-scope atomic, a device `publish` is the slot protocol, and both are real work
//! that the host model cannot check, so they are not claimed here. `global_release` is false for
//! the same reason — a cohort-wide barrier needs a cooperative launch and this backend does not
//! require one.

use crate::contract::{
    Backend, BackendFault, Channel, Deployment, Edge, Error, Failure, FailureKind, Frame,
    FrameBytes, Invalid, Rank, Tag,
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

pub const ID: Backend = Backend::Nv;

/// The longest frame either route carries: a 64 KiB log block behind its eight-byte header.
///
/// Provisioned, not discovered. Peer links are slots of the launch's arena, whose `Layout` the
/// launch fixes before entry and `init` refuses when a slot holds less; the leader route's slots
/// are `leader::CAPACITY`, which is this number, in the region the launch sizes by `Route::words`.
pub const MAX_FRAME: FrameBytes = FrameBytes::new(65_544);

/// Alignment is one byte: a device has no pages, and aligning for a mapping that will not happen
/// would shrink every share for nothing.
pub fn align() -> usize {
    1
}

/// Slots per destination element, the rule `affected` is read against.
///
/// The launch's arena has one depth for every pair, so this is the factor the *widest* pair needs
/// rather than a per-pair depth; `reshape` refuses a declaration the launched arena cannot hold.
pub const FACTOR: u64 = 4;

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
    /// The cohort: every worker of the launch, in order. The domain `share` partitions by.
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
        self.leader.as_mut().ok_or(Error::Invalid(Invalid::NoLeader))
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
    let refuse = |participant, fault| Failure {
        participant,
        operation: "init",
        kind: FailureKind::Backend(fault),
    };
    let outside = BackendFault::Invalid(Invalid::RankOutsideJob);
    // The contract's participant set is the workers, and every one must be a rank this launch
    // started and the transport table can hold.
    let launch = deployment.workers();
    if launch.len() > MAX_RANKS {
        return Err(refuse(env.rank, BackendFault::Storage));
    }
    if launch.iter().any(|worker| worker.get() >= env.size) {
        return Err(refuse(env.rank, outside));
    }
    let count = launch.len();
    let mut all = [Rank::from_index(0); MAX_RANKS];
    all[..count].copy_from_slice(launch);
    // The rank the contract reports is the position in the declaration; the number the launch
    // knows this participant by is the one in the table. They coincide unless the launch said
    // otherwise, and when it does, this is the one place the difference exists.
    let rank = deployment
        .contract(env.rank)
        .ok_or(refuse(env.rank, outside))?;
    let _colour = cohort(rank, launch);
    // Every peer-link slot must hold a `MAX_FRAME` frame, and the arena is fixed before entry, so
    // a launch that provisioned less is refused here rather than at its first long frame.
    if env.fabric.layout().capacity() < MAX_FRAME.get() {
        return Err(refuse(rank, BackendFault::Storage));
    }

    // `Links::open` refuses only a rank outside the launch.
    let links =
        Links::open(&env.fabric, env.rank.get(), env.size).map_err(|_| refuse(rank, outside))?;

    let mut hosts = [Rank::from_index(0); MAX_RANKS];
    // Every rank of a launch shares the device's memory, so every rank's leader is the first. The
    // table is one entry per rank and not one entry, which would say the launch has one rank.
    for slot in hosts.iter_mut().take(count) {
        *slot = launch[0];
    }

    // The worker end of the leader route, when the declaration names a leader for this worker. The
    // geometry comes from the declaration, not from the environment: both ends must agree on it and
    // the worker list is the one thing they both hold.
    let leader = match deployment.leader_of(rank) {
        None => None,
        Some(_) => {
            if env.leader_region.is_null() {
                return Err(refuse(rank, BackendFault::Storage));
            }
            let route =
                Route::sized(count as u32).map_err(|_| refuse(rank, BackendFault::Storage))?;
            // SAFETY: the launch prepared this region for exactly this route and owns it for the
            // life of the job; this worker is the only producer on its up link and the only
            // consumer of its down link.
            Some(unsafe { leader::Worker::new(env.leader_region, route, rank.get()) })
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

fn cohort(cx: &Context) -> &[Rank] {
    &cx.cohort[..cx.size as usize]
}

/// Report the outcome and discharge nothing.
///
/// A participant that is not a process ends by returning; what ends the launch is the kernel
/// returning, which is the launcher's business and not this call's. The outcome travels back
/// because a failed run has to be to be reported rather than completed.
pub fn done<A>(_cx: &mut Context, outcome: Result<(), Failure<A>>) -> Result<(), Failure<A>> {
    outcome
}

// ---------------------------------------------------------------------------------------------
// Communication

/// A refusal the link layer cannot give for this operation: a send told `Empty`, a receive told
/// `Full`. Not the caller's input, so it is the backend's own fault.
fn internal(cx: &Context, operation: &'static str) -> Error {
    Error::Failed(Failure {
        participant: cx.rank,
        operation,
        kind: FailureKind::Backend(BackendFault::Internal),
    })
}

/// Send one frame, in one attempt. Success means *accepted*: the bytes are in the peer's slot or
/// nowhere.
pub fn send(cx: &mut Context, to: Rank, channel: Channel, data: &[u8]) -> Result<(), Error> {
    let tag = match channel {
        Channel::Message(tag) => tag,
        Channel::Lane => cx
            .tag
            .ok_or(Error::Invalid(Invalid::LaneNotConfigured))?,
    };
    match peers::try_send(&mut cx.links, to.get(), u32::from(tag.get()), data) {
        Ok(()) => Ok(()),
        Err(Refused::Full) => Err(Error::Full),
        Err(Refused::TooLarge) => Err(Error::TooLarge {
            limit: FrameBytes::new(cx.layout.capacity()),
        }),
        Err(Refused::NoSuchPeer) => Err(Error::Invalid(Invalid::RankOutsideJob)),
        Err(Refused::Empty | Refused::TooSmall { .. }) => Err(internal(cx, "send")),
    }
}

/// Take one frame from any source, into the caller's buffer, in one pass over the sources.
///
/// A frame that does not fit is reported as `TooSmall` and is not consumed, so the caller may
/// grow its buffer and ask again. Nothing is stashed: demultiplexing by tag is the caller's job,
/// and a backend that held frames for a tag nobody asked about would be a queue whose depth
/// nobody declared.
pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    let mut needed = 0u32;
    for src in 0..cx.size {
        match peers::try_recv(&mut cx.links, src, out) {
            Ok(Message { src, tag, len }) => {
                // The tag word was written from a `u16` by the link's only producer.
                return Ok(Some(Frame::new(
                    Rank::from_index(src),
                    Tag::new(tag as u16),
                    FrameBytes::new(len),
                )));
            }
            Err(Refused::TooSmall { needed: n }) => needed = needed.max(n),
            // A source with nothing waiting, and a source that is not a participant at all, are
            // both "no frame here"; the destination check belongs to a send.
            Err(Refused::Empty | Refused::NoSuchPeer) => {}
            Err(Refused::Full | Refused::TooLarge) => return Err(internal(cx, "recv")),
        }
    }
    if needed > 0 {
        return Err(Error::TooSmall {
            needed: FrameBytes::new(needed),
        });
    }
    Ok(None)
}

/// Report that every accepted send has released its send resource.
///
/// Always immediate on this backend, and not as a convenience: a device send copies into the
/// peer's slot and publishes it before it returns, so there is no operation-specific resource
/// left outstanding for a flush to wait on. What this does *not* mean is that the receiver has
/// read the frame — completion is local and operation-specific, and a caller wanting to know that
/// the far side consumed something is asking for a credit scheme this backend does not have.
pub fn flush(_cx: &mut Context) -> Result<(), Error> {
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
    bytes: FrameBytes,
    tag: Tag,
) -> Result<(), Error> {
    if workers.windows(2).any(|w| w[0] >= w[1]) {
        return Err(Error::Invalid(Invalid::UnorderedWorkers));
    }
    if workers.iter().any(|&w| w.get() >= cx.size) {
        return Err(Error::Invalid(Invalid::RankOutsideJob));
    }
    if edges.windows(2).any(|w| {
        (w[0].source(), w[0].destination()) >= (w[1].source(), w[1].destination())
    }) {
        return Err(Error::Invalid(Invalid::UnorderedEdges));
    }
    for edge in edges {
        if !workers.contains(&edge.source()) || !workers.contains(&edge.destination()) {
            return Err(Error::Invalid(Invalid::EdgeOutsideWorkers));
        }
    }
    if bytes.get() > cx.layout.capacity() {
        return Err(Error::TooLarge {
            limit: FrameBytes::new(cx.layout.capacity()),
        });
    }
    if edges
        .iter()
        .map(|edge| u64::from(edge.affected().get()) * FACTOR)
        .max()
        .is_some_and(|needed| needed > u64::from(cx.layout.depth()))
    {
        return Err(Error::Invalid(Invalid::UnsupportedGeometry));
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

/// Publish this participant's slice into the launch's segment.
pub fn share(cx: &mut Context, mine: &[u8], total: usize) -> Result<Shared, Error> {
    if cx.segment.is_null() || total > cx.segment_bytes {
        return Err(Error::Invalid(Invalid::MissingSegment));
    }
    let crate::contract::ByteRange { offset, length } =
        crate::partition::slice_of(hosts(cx), cohort(cx), cx.rank, total)
            .map_err(Error::Invalid)?;
    if mine.len() != length {
        return Err(Error::Invalid(Invalid::BadShareLength));
    }
    // SAFETY: the environment's contract is that `segment` addresses `segment_bytes` writable
    // bytes for the launch's life, and `slice_of` returned a range inside `total <= segment_bytes`.
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
pub fn unshare(_cx: &mut Context, _segment: Shared) -> Result<(), (Shared, Error)> {
    Ok(())
}

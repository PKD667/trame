//! The device backend: one CUDA warp per participant.
//!
//! It uses none of the shared MPI environment, so it supplies the whole surface itself, as
//! `none/` does. Everything that is a fact about the device lives in the submodules: the links,
//! the lane and its split, and the clock. Everything that is a fact about the *contract* is here.
//!
//! # Participant-local state, and why there is no global
//!
//! The entry rules forbid an implicit process-global context, and on a device every warp of a
//! launch would share one: the last writer's rank and links would be every warp's. Identity and
//! the endpoints are fields of [`Context`], which the entry builds once and the
//! caller owns, and `peers::Links` holds the per-link sequence numbers.
//!
//! The launch description `Environment::default` reads (`launch`) is a global of another kind: the
//! launcher writes it before any participant runs, no participant writes it, and it states only
//! what every participant of the launch shares. A warp's identity is its own index, never a field
//! of it.
//!
//! # What a device changes about the surface
//!
//! * **One device, so one sharing domain.** `hosts` is rank 0 for every rank.
//! * **Every link has the launch's layout.** The arena is allocated before any graph is known, so
//!   one geometry covers every pair, and `reshape` refuses a declaration whose implied depth or
//!   frame size the launched arena cannot serve. Refusing at declaration is required: a frame
//!   that does not fit must never be truncated at run time.
//! * **Both channels ride the links.** A device has no separate message wire; `Message` and `Lane`
//!   differ in the tag they carry and in what the caller may assume, not in the memory they use.
//!   Lanes are acknowledged writes into the receiver's own slot, so they do not lose, and the
//!   `Message` route's reliability is therefore inherited rather than promised separately.
//! * **`sync` is warp-wide.** `Exclusive` and `Handoff` keep `cpu::sync`'s signatures; under
//!   `cuda` the whole warp calls each method and one lane acts for it (see `sync`).
//! * **Collectives wait at a barrier in the arena.** Two words after the link rings, which the
//!   launcher zeroes (`peers::BARRIER`), count arrivals at entry and at lane declaration/retirement
//!   so no worker sees an unfinished cohort.

use crate::contract::{
    Backend, BackendFault, Channel, Deployment, Edge, Error, Failure, FailureKind, Frame, Handle,
    Invalid, Launch, Participant, Rank, Tag,
};

pub mod clock;
pub mod device;
pub mod error;
pub mod launch;
pub mod layout;
pub mod leader;
pub mod model;
pub mod peers;
pub mod run;
pub use run::concurrent_io;
pub mod sync;
pub mod transport;
pub mod warp;

#[cfg(test)]
mod tests;
// The rings measured on a GPU. Test-only, so their kernels stay out of the library.
#[cfg(all(test, feature = "cuda"))]
mod measure;

use core::marker::PhantomData;

use crate::invoke::Owner;
use layout::Layout;
use leader::Route;
use peers::{Links, Refused};
use transport::{MAX_RANKS, Message};

pub const ID: Backend = Backend::Nv;

/// The longest frame either route carries: a 64 KiB log block behind its eight-byte header.
///
/// Provisioned, not discovered. Peer links are slots of the launch's arena, whose `Layout` the
/// launch fixes before entry and `init` refuses when a slot holds less; the leader route's slots
/// are `leader::CAPACITY`, which is this number, in the region the launch sizes by `Route::words`.
pub const MAX_FRAME: usize = 65_544;

/// Slots per destination element, the rule `affected` is read against.
///
/// The launch's arena has one depth for every pair, so this is the factor the *widest* pair needs
/// rather than a per-pair depth; `reshape` refuses a declaration the launched arena cannot hold.
pub const FACTOR: u64 = 4;

// A Rust `bool` inside `Context` makes cuda-oxide refuse the pointer-bearing `Result` from
// `init`: its enum slot map cannot preserve provenance across that overlapping layout. Keep
// device-resident flags as bytes; 0 and 1 are the only values written here.
type Bool = u8;

/// What the launch supplies, discovered rather than handed down.
///
/// `default` reads the launch description the launcher wrote before any participant ran, as MPI's
/// participants discover their world; `init` and `Leader::open` check it before any of it is used,
/// so a launch that wrote nothing is refused there. Nothing in it is public, because what a launch
/// states is the launcher's business and a host that named it would know which backend it runs.
///
/// A clone is the same description: it names the launch's memory and frees nothing.
#[derive(Clone)]
pub struct Environment {
    description: launch::Description,
}

impl Default for Environment {
    fn default() -> Self {
        Environment {
            description: launch::read(),
        }
    }
}

/// This participant's state. Owned by the caller, passed by `&mut`, shared with nobody.
///
/// `repr(C)` so that its first 32 bytes are integers. `init` returns it in `Result<Context,
/// Failure>`, whose `Err` rustc lays over the start of a `Context`, and cuda-oxide refuses to lower
/// an enum whose variants lay a pointer over bytes that are not the same pointer; in rustc's own
/// order a leader endpoint's pointer lay under a `Failure`'s participant.
#[repr(C)]
pub struct Context {
    rank: Rank,
    size: u32,
    /// Each contract rank's launch rank: the links are the launch's, so this is where a contract
    /// rank becomes a link index and a link index becomes a contract rank again.
    launch: [Launch; MAX_RANKS],
    links: Links,
    /// The worker's end of the leader route, when the launch named a leader.
    leader: Option<leader::Worker>,
    layout: Layout,
    /// The tag lane traffic rides under, set by `reshape`.
    tag: Option<Tag>,
    /// The declared outgoing edges and frame bound: a lane send may only use the current load.
    lanes: [Bool; MAX_RANKS],
    lane_frame: usize,
    /// One device: every rank's sharing-domain leader. Held here rather than in a static because
    /// a static is what made two participants share one identity.
    hosts: [Rank; MAX_RANKS],
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

// ---------------------------------------------------------------------------------------------
// Entry and failure

/// Enter the launch.
pub fn init(env: Environment, deployment: Deployment<'_>) -> Result<Context, Failure> {
    let refuse = |participant, fault| Failure {
        participant,
        operation: named(b"init"),
        kind: FailureKind::Backend(fault),
    };
    let outside = BackendFault::Invalid(Invalid::RankOutsideJob);
    // A worker is its warp: the launch-wide description cannot say which one this is.
    let here = Launch::new(warp::here_id());
    let unentered = Participant::Entering(Some(here));
    // A `match`, not `map_err`: a `Result<Launched, Failure>` lays a `Failure` over the fabric's
    // pointer, which cuda-oxide refuses to lower (see `Context`).
    let env = match env.description.check() {
        Ok(env) => env,
        Err(fault) => return Err(refuse(unentered, fault)),
    };
    if here.get() >= env.size {
        return Err(refuse(unentered, outside));
    }
    // The contract's participant set is the workers, and every one must be a rank this launch
    // started and the transport table can hold.
    let launch = deployment.workers();
    if launch.len() > MAX_RANKS {
        return Err(refuse(unentered, BackendFault::Storage));
    }
    if launch.iter().any(|worker| worker.get() >= env.size) {
        return Err(refuse(unentered, outside));
    }
    let count = launch.len();
    let mut launched = [Launch::new(0); MAX_RANKS];
    launched[..count].copy_from_slice(launch);
    // The rank the contract reports is the position in the declaration; the number the launch
    // knows this participant by is the one in the table. This is the launch-to-contract half of
    // the one conversion pair.
    let rank = contract_of(&launched[..count], here).ok_or(refuse(unentered, outside))?;
    let me = Participant::Worker(rank);
    // Every peer-link slot must hold a `MAX_FRAME` frame, and the arena is fixed before entry, so
    // a launch that provisioned less is refused here rather than at its first long frame.
    if (env.fabric.layout().capacity() as usize) < MAX_FRAME {
        return Err(refuse(me, BackendFault::Storage));
    }

    // `Links::open` refuses only a rank outside the launch. A `match` for the reason `check`'s is.
    let mut links = match Links::open(&env.fabric, here.get(), env.size) {
        Ok(links) => links,
        Err(_) => return Err(refuse(me, outside)),
    };
    let hosts = [Rank::from_index(0); MAX_RANKS];
    // Every rank of a launch shares the device's memory, so every rank's leader is contract rank
    // zero. The table is one entry per rank and not one entry, which would say the launch has one
    // rank.

    // The worker end of the leader route, when the declaration names a leader for this worker. The
    // geometry comes from the declaration, not from the environment: both ends must agree on it and
    // the worker list is the one thing they both hold.
    // One device launch has one host leader. A deployment naming another process cannot use
    // this region: accepting it would silently connect a worker to the wrong leader.
    if deployment
        .leaders()
        .is_some_and(|leaders| leaders.iter().any(|&leader| Some(leader) != env.leader))
    {
        return Err(refuse(me, outside));
    }
    let leader = match deployment.leader_of(rank) {
        None => None,
        Some(_) => {
            if env.leader_region.is_null() {
                return Err(refuse(me, BackendFault::Storage));
            }
            // A `match`: a `Result<Route, Failure>` lays the name's pointer over the route's layout.
            let route = match Route::sized(count as u32) {
                Ok(route) => route,
                Err(_) => return Err(refuse(me, BackendFault::Storage)),
            };
            if env.leader_words != route.words() {
                return Err(refuse(me, BackendFault::Storage));
            }
            // SAFETY: the launcher's write of the description vouched that this region is
            // `leader_words` words prepared for this route and owned for the life of the job, and
            // that is the route's size; this worker is the only producer on its up link and the
            // only consumer of its down link.
            Some(unsafe { leader::Worker::new(env.leader_region, route, rank.get()) })
        }
    };

    // Entry is a worker collective, not merely a local read of the launch description.
    peers::barrier(&mut links, count as u32);
    Ok(Context {
        rank,
        size: count as u32,
        launch: launched,
        links,
        leader,
        layout: env.fabric.layout(),
        tag: None,
        lanes: [0; MAX_RANKS],
        lane_frame: 0,
        hosts,
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

/// An operation's name, as the `&'static str` a `Failure` carries.
///
/// It exists because cuda-oxide has no device translation for a `&str` constant (its issue #76):
/// a string literal in code a kernel reaches does not compile, and a byte string does. Not inlined,
/// so the conversion is not folded back into the `&str` constant it avoids.
#[inline(never)]
fn named(name: &'static [u8]) -> &'static str {
    // SAFETY: every caller passes an ASCII byte-string literal.
    unsafe { core::str::from_utf8_unchecked(name) }
}

/// A refusal the link layer cannot give for this operation: a send told `Empty`, a receive told
/// `Full`. Not the caller's input, so it is the backend's own fault.
fn internal(cx: &Context, operation: &'static str) -> Error {
    Error::Failed(Failure {
        participant: Participant::Worker(cx.rank),
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
    let link = launch_of(&cx.launch[..cx.size as usize], to)?.get();
    let limit = match channel {
        Channel::Message(_) => MAX_FRAME,
        Channel::Lane => {
            if cx.lanes[to.get() as usize] == 0 {
                return Err(Error::Invalid(Invalid::NoLane));
            }
            cx.lane_frame
        }
    };
    if data.len() > limit {
        return Err(Error::TooLarge { limit });
    }
    match peers::try_send(&mut cx.links, link, u32::from(tag.get()), data) {
        Ok(()) => Ok(()),
        Err(Refused::Full) => Err(Error::Full),
        Err(Refused::NoSuchPeer) => Err(Error::Invalid(Invalid::RankOutsideJob)),
        // `init` refused a slot smaller than `MAX_FRAME`, so a slot refusing a frame this size is
        // the backend's own fault.
        Err(Refused::TooLarge | Refused::Empty | Refused::TooSmall { .. }) => {
            Err(internal(cx, named(b"send")))
        }
    }
}

/// A contract rank's launch rank: the link index the launch's arena is addressed by. The one
/// place a contract rank becomes a launch rank here.
fn launch_of(launched: &[Launch], contract: Rank) -> Result<Launch, Error> {
    launched
        .get(contract.get() as usize)
        .copied()
        .ok_or(Error::Invalid(Invalid::RankOutsideJob))
}

/// A launch rank's contract rank, the one place a link index becomes a participant again. This is
/// where a frame's `src` is translated, so a link index is never mistaken for a contract rank.
fn contract_of(launched: &[Launch], launch: Launch) -> Option<Rank> {
    launched
        .iter()
        .position(|&l| l == launch)
        .map(|at| Rank::from_index(at as u32))
}

/// The receive room a link is told about: no frame exceeds `MAX_FRAME`, so a longer buffer is
/// offered as `MAX_FRAME` bytes and the link never sees a length it would have to narrow.
fn room(out: &mut [u8]) -> &mut [u8] {
    let room = out.len().min(MAX_FRAME);
    &mut out[..room]
}

/// Take one frame from any source, into the caller's buffer, in one pass over the sources.
///
/// A frame that does not fit is reported as `TooSmall` and is not consumed, so the caller may
/// grow its buffer and ask again. Nothing is stashed: demultiplexing by tag is the caller's job,
/// and a backend that held frames for a tag nobody asked about would be a queue whose depth
/// nobody declared.
pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    take(cx, Owner::ALL, out)
}

/// The next peer frame `owner` owns. A link delivers in order, so a head another arm owns holds
/// that link for this one.
fn take(cx: &mut Context, owner: Owner<'_>, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    let mut needed = 0u32;
    let out = room(out);
    for source in 0..cx.size {
        let launch = cx.launch[source as usize];
        if !owner.every()
            && !peers::head(&mut cx.links, launch.get()).is_some_and(|tag| owner.owns(Tag::new(tag as u16)))
        {
            continue;
        }
        match peers::try_recv(&mut cx.links, launch.get(), out) {
            Ok(Message { tag, len, src }) => {
                // The tag word was written from a `u16` by the link's only producer. The source
                // word is a launch rank, so it becomes a contract rank here and nowhere else.
                return Ok(Some(Frame::new(
                    Some(
                        contract_of(&cx.launch[..cx.size as usize], Launch::new(src))
                            .ok_or(Error::Invalid(Invalid::RankOutsideJob))?,
                    ),
                    Tag::new(tag as u16),
                    len as usize,
                )));
            }
            Err(Refused::TooSmall { needed: n }) => needed = needed.max(n),
            Err(Refused::Empty) => {}
            Err(Refused::Full | Refused::TooLarge | Refused::NoSuchPeer) => {
                return Err(internal(cx, named(b"recv")));
            }
        }
    }
    if needed > 0 {
        return Err(Error::TooSmall {
            needed: needed as usize,
        });
    }
    Ok(None)
}

/// One `concurrent!` arm's end of this participant's routes, built for one step. The arms run one
/// after another on the warp, so each borrows the whole context while it steps.
pub struct Io<'a> {
    cx: &'a mut Context,
    owner: Owner<'a>,
    /// Whether the next receive tries the leader first. The arm's, kept across its steps.
    leader_first: &'a mut bool,
}

impl<'a> Io<'a> {
    pub(crate) fn new(cx: &'a mut Context, owner: Owner<'a>, leader_first: &'a mut bool) -> Self {
        Io { cx, owner, leader_first }
    }
}

impl Io<'_> {
    pub fn send(&mut self, to: Rank, channel: Channel, data: &[u8]) -> Result<(), Error> {
        send(self.cx, to, channel, data)
    }

    pub fn lead(&mut self, tag: Tag, data: &[u8]) -> Result<(), Error> {
        leader::send(self.cx, tag, data)
    }

    /// The first frame this arm owns, from a peer or its leader, alternating which goes first.
    pub fn recv(&mut self, out: &mut [u8]) -> Result<Option<Frame>, Error> {
        if !self.owner.receives() {
            return Err(Error::Invalid(Invalid::NotReceiving));
        }
        let first = *self.leader_first;
        *self.leader_first = !first;
        for from_leader in [first, !first] {
            let frame = match from_leader {
                true if self.cx.leader.is_none() => None,
                true => leader::take(self.cx, self.owner, out)?,
                false => take(self.cx, self.owner, out)?,
            };
            if frame.is_some() {
                return Ok(frame);
            }
        }
        Ok(None)
    }

    pub fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
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
    bytes: usize,
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
    if bytes > MAX_FRAME {
        return Err(Error::TooLarge { limit: MAX_FRAME });
    }
    if edges
        .iter()
        .map(|edge| u64::from(edge.affected().get()) * FACTOR)
        .max()
        .is_some_and(|needed| needed > u64::from(cx.layout.depth()))
    {
        return Err(Error::Invalid(Invalid::UnsupportedGeometry));
    }

    cx.lanes.fill(0);
    for edge in edges {
        if edge.source() == cx.rank {
            cx.lanes[edge.destination().get() as usize] = 1;
        }
    }
    cx.lane_frame = bytes;
    cx.tag = Some(tag);
    peers::barrier(&mut cx.links, cx.size);
    Ok(())
}

/// Retire the caller's lane table only after every worker has left this load's lane use.
pub fn release(cx: &mut Context) -> Result<(), Error> {
    peers::barrier(&mut cx.links, cx.size);
    cx.lanes.fill(0);
    cx.lane_frame = 0;
    cx.tag = None;
    Ok(())
}

/// A phase boundary over the entered workers: the arena barrier `release` uses, without touching
/// the lane table. It consumes no link frame and does not finalize sends.
pub fn barrier(cx: &mut Context) {
    peers::barrier(&mut cx.links, cx.size);
}

/// No value exists: publication is not implemented on a device yet (`backend.md`).
pub struct Shared {
    never: core::convert::Infallible,
    _local: PhantomData<*const ()>,
}

/// Attach the segment named by `handle`.
///
/// # Safety
/// `handle` came from `leader::handle` of a segment not retired before the returned `Shared` is
/// detached or dropped.
pub unsafe fn attach(cx: &mut Context, _handle: Handle) -> Result<Shared, Error> {
    Err(Error::Failed(Failure {
        participant: Participant::Worker(cx.rank),
        operation: named(b"attach"),
        kind: FailureKind::Backend(BackendFault::Unimplemented),
    }))
}

/// The segment, read-only.
pub fn bytes(segment: &Shared) -> &[u8] {
    match segment.never {}
}

/// Retire this worker's mapping.
pub fn detach(_cx: &mut Context, segment: Shared) -> Result<(), (Shared, Error)> {
    match segment.never {}
}

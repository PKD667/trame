// The MPI environment and the participant it belongs to.
//
// This was a process-wide `OnceLock` with free functions answering from it. Every function now
// takes the participant's state, because the entry rules forbid an implicit global context: two
// participants in one process — a test, or a host service — would have shared the first one's rank
// and communicators, and neither could have told. The record is a value `init` returns and the
// caller owns.
//
// One record for all three MPI backends, because they are three lane transports over one MPI
// environment and the environment's lifetime is the participant's. What differs between them is
// how `reshape` builds the lane window and how `lane` drives it, and that lives in each backend.

#[cfg(feature = "ring")]
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
#[cfg(feature = "ring")]
use std::sync::Arc;

use mpi::collective::CommunicatorCollectives;
use mpi::environment::Universe;
use mpi::raw::traits::AsRaw;
use mpi::topology::{Color, Communicator, InterCommunicator, SimpleCommunicator};

#[cfg(feature = "ring")]
use mpi_rma::Ring;

#[cfg(feature = "ring")]
use crate::contract::Edge;
use crate::contract::Frame;
use crate::invoke::{Owner, Receive};
use crate::contract::{
    BackendFault, Deployment, Error, Failure, FailureKind, Invalid, Launch, Participant, Rank, Tag,
};

/// Megabytes attached for buffered sends, when the entry says nothing.
const BSEND_MB: usize = 128;

/// The longest frame: half the default attached buffer. Every send is buffered, so a frame must fit
/// the attached buffer beside MPI's per-message bookkeeping, whose size MPI does not state; the
/// other half is that margin. `init` and `Leader::open` refuse a smaller buffer.
pub const MAX_FRAME: usize = (BSEND_MB << 20) / 2;

/// The smallest attached buffer an entry may state.
const BSEND_MIN: usize = 2 * MAX_FRAME;

/// The MPI environment supplied at entry.
///
/// The attached buffer has one fixed size: MPI discovers its own world, while this value carries
/// the buffer size the backend needs for buffered sends. It owns no MPI object — `init` and
/// `Leader::open` each enter `MPI_COMM_WORLD` and the `Universe` lives in the returned `Context`.
#[derive(Clone)]
pub struct Environment {
    bsend_bytes: usize,
}

impl Default for Environment {
    /// What an entry that states nothing gets: the attached buffer at its documented default.
    ///
    /// A host binding calls this rather than naming the field, because the field is a transport's
    /// business and a host that named it would be a host that knew which backend it was compiled
    /// against — which is the coupling the entry rules exist to prevent.
    fn default() -> Self {
        Environment {
            bsend_bytes: BSEND_MB << 20,
        }
    }
}

/// The lane table and window a load declared.
///
/// Empty between loads. `mpi` never opens a window — its lanes are ordinary frames — so `ring`
/// stays `None` there and the tag alone is the lane route.
pub(crate) struct Lane {
    tag: Tag,
    workers: Vec<Rank>,
    #[cfg(feature = "ring")]
    ring: Option<Arc<Ring>>,
    /// Frames a window poll took and no receive has handed over yet. A poll acknowledges every
    /// frame it returns, so these are accepted and owned here until a caller takes each one.
    #[cfg(feature = "ring")]
    taken: VecDeque<(Rank, Vec<u8>)>,
}

impl Lane {
    fn none() -> Lane {
        Lane {
            tag: Tag::new(0),
            workers: Vec::new(),
            #[cfg(feature = "ring")]
            ring: None,
            #[cfg(feature = "ring")]
            taken: VecDeque::new(),
        }
    }
}

/// This participant's MPI state. Owned by the caller, passed by `&mut`, shared with nobody.
pub struct Context {
    /// Held, never read: dropping a `Universe` finalizes MPI, and `done` is what finalizes.
    _universe: ManuallyDrop<Universe>,
    /// Held as options so `done` can release them before finalising. The entry rules give `done` a
    /// `&mut`, so it cannot consume the record, and freeing a communicator after `MPI_Finalize` is
    /// disallowed — a rank that does it aborts the job instead of reporting a failure.
    world: Option<SimpleCommunicator>,
    together: Option<SimpleCommunicator>,
    rank: Rank,
    size: u32,
    hosts: Vec<Rank>,
    /// The worker's end of the bridge to its leader, when the deployment named one.
    ///
    /// A different communicator of a different *kind*, which is why it is a second field rather
    /// than a rank the message route could also have used: an inter-communicator addresses a remote
    /// group, and folding it into the message route would be treating two kinds of thing as one.
    leader: Option<InterCommunicator>,
    /// The worker subgroup used to construct the bridge; released after the bridge.
    leader_group: Option<SimpleCommunicator>,
    lane: Lane,
    // Auto traits match the nv backend's, so the public surface is the same on every backend.
    _unshared: PhantomData<*const ()>,
}

/// # Safety
///
/// MPI is initialised with `MPI_THREAD_MULTIPLE` and a shortfall is refused by `init`, so
/// concurrent calls from different threads on one communicator are safe at this threading level.
/// The `Universe` is not `Send` because its `Drop` finalizes; this record never drops it.
unsafe impl Send for Context {}

impl Context {
    pub(crate) fn world(&self) -> &SimpleCommunicator {
        self.world
            .as_ref()
            .expect("the communicators were released by `done`")
    }

    /// The cohort's communicator. Built once by `init` and reused by every collective, because
    /// building one per load would carry the group over point-to-point on the parent communicator
    /// while peers are probing it for any tag.
    pub(crate) fn together(&self) -> &SimpleCommunicator {
        self.together
            .as_ref()
            .expect("the communicators were released by `done`")
    }

    pub(crate) fn rank(&self) -> Rank {
        self.rank
    }

    /// Take the lane table. Only the tag and the worker list: a backend whose lanes are ordinary
    /// frames has nothing else, and one whose lanes are not calls [`set_window`](Self::set_window).
    // Only the lane transport that sends lanes as tagged frames reads these.
    #[cfg_attr(feature = "ring", allow(dead_code))]
    pub(crate) fn set_lane(&mut self, tag: Tag, workers: Vec<Rank>) {
        self.lane = Lane {
            tag,
            workers,
            ..Lane::none()
        };
    }

    /// Take the lane table together with the window that carries it. Ring builds only, because a
    /// window is what `mpi-rma` supplies and it is not linked otherwise.
    #[cfg(feature = "ring")]
    pub(crate) fn set_window(&mut self, tag: Tag, workers: Vec<Rank>, ring: Arc<Ring>) {
        self.lane = Lane {
            tag,
            workers,
            ring: Some(ring),
            taken: VecDeque::new(),
        };
    }

    /// The window the current load opened, if any. Ring builds only.
    #[cfg(feature = "ring")]
    pub(crate) fn ring(&self) -> Option<Arc<Ring>> {
        self.lane.ring.clone()
    }

    pub(crate) fn clear_lane(&mut self) {
        self.lane = Lane::none();
    }

    /// The lane's tag, or `Invalid` when no load declared one.
    // Only the lane transport that sends lanes as tagged frames reads these.
    #[cfg_attr(feature = "ring", allow(dead_code))]
    pub(crate) fn lane_tag(&self) -> Result<Tag, Error> {
        lane_tag(&self.lane.workers, self.lane.tag)
    }

    /// This participant's position in the lane table, which is what the window is indexed by.
    #[cfg(feature = "ring")]
    pub(crate) fn lane_index(&self, rank: Rank) -> Result<i32, Error> {
        lane_index(&self.lane.workers, rank)
    }

    /// One attempt at the next lane frame. See [`lane_frame`].
    #[cfg(feature = "ring")]
    pub(crate) fn next_lane_frame(&mut self, out: &mut [u8]) -> Result<Option<Frame>, Error> {
        let Some(ring) = self.lane.ring.clone() else {
            return Ok(None);
        };
        let lane = &mut self.lane;
        lane_frame(&ring, &mut lane.taken, &lane.workers, lane.tag, self.rank, out)
    }

    /// The leader route, or `None` when the deployment named no leader.
    pub(crate) fn leader_route(&self) -> Option<&InterCommunicator> {
        self.leader.as_ref()
    }

    pub(crate) fn failure(&self, operation: &'static str) -> Error {
        failure(self.rank, operation)
    }
}

/// A transport failure `rank` observed in `operation`.
pub(crate) fn failure(rank: Rank, operation: &'static str) -> Error {
    Error::Failed(Failure {
        participant: Participant::Worker(rank),
        operation,
        kind: FailureKind::Backend(BackendFault::Transport),
    })
}

/// A lane table's tag, or `Invalid` when no load declared one.
#[cfg_attr(feature = "ring", allow(dead_code))]
pub(crate) fn lane_tag(workers: &[Rank], tag: Tag) -> Result<Tag, Error> {
    if workers.is_empty() {
        return Err(Error::Invalid(Invalid::LaneNotConfigured));
    }
    Ok(tag)
}

/// `rank`'s position in the lane table, which is what the window is indexed by.
///
/// `Rank` is a world rank and a window is indexed by position among the workers, so the two
/// are not the same number and the mapping is not the identity.
#[cfg(feature = "ring")]
pub(crate) fn lane_index(workers: &[Rank], rank: Rank) -> Result<i32, Error> {
    let at = workers
        .iter()
        .position(|&worker| worker == rank)
        .ok_or(Error::Invalid(Invalid::NoLane))?;
    i32::try_from(at).map_err(|_| Error::Invalid(Invalid::Unrepresentable))
}

/// The oldest frame already taken from the window, or, when none is, one poll of it that
/// acknowledges what it took. A frame that does not fit stays first in line.
#[cfg(feature = "ring")]
fn lane_frame(
    ring: &Ring,
    taken: &mut VecDeque<(Rank, Vec<u8>)>,
    workers: &[Rank],
    tag: Tag,
    me: Rank,
    out: &mut [u8],
) -> Result<Option<Frame>, Error> {
    if taken.is_empty() {
        let messages = ring.poll().map_err(|_| failure(me, "poll"))?;
        // The last sequence per origin: one cumulative ack each.
        let mut last: Vec<(i32, u64)> = Vec::new();
        for message in messages {
            match last.iter_mut().find(|(origin, _)| *origin == message.origin) {
                Some(seen) => seen.1 = message.sequence,
                None => last.push((message.origin, message.sequence)),
            }
            let source = usize::try_from(message.origin)
                .ok()
                .and_then(|at| workers.get(at).copied())
                .ok_or(failure(me, "recv"))?;
            taken.push_back((source, message.data));
        }
        // The frames are owned in `taken`, so their slots are free. A raw ring's ack is a no-op.
        for (origin, sequence) in last {
            ring.ack(origin, sequence).map_err(|_| failure(me, "ack"))?;
        }
    }
    let Some((source, data)) = taken.front() else {
        return Ok(None);
    };
    let len = data.len();
    if len > out.len() {
        return Err(Error::TooSmall { needed: len });
    }
    out[..len].copy_from_slice(data);
    let frame = Frame::new(Some(*source), tag, len);
    taken.pop_front();
    Ok(Some(frame))
}

/// One `concurrent!` arm's end of this participant's routes. The arms share the communicators,
/// which `MPI_THREAD_MULTIPLE` permits, and only the arm that owns the lane tag polls the window.
pub struct Io<'a> {
    pub(crate) world: &'a SimpleCommunicator,
    pub(crate) leader: Option<&'a InterCommunicator>,
    pub(crate) rank: Rank,
    /// The lane table: the tag lane frames carry and the workers the window is indexed by.
    pub(crate) lane: Tag,
    pub(crate) workers: &'a [Rank],
    #[cfg(feature = "ring")]
    pub(crate) ring: Option<&'a Ring>,
    #[cfg(feature = "ring")]
    taken: Option<&'a mut VecDeque<(Rank, Vec<u8>)>>,
    owner: Owner<'a>,
    /// Where the next receive starts: a route, and a tag within each probing route's list.
    next: usize,
    turn: [usize; 2],
    // Auto traits match the nv backend's, so the public surface is the same on every backend.
    _unshared: PhantomData<*const ()>,
}

// SAFETY: `init` refuses less than `MPI_THREAD_MULTIPLE`, so every arm's thread may call the
// shared communicators, and the window keeps its own state behind its locks.
unsafe impl Send for Io<'_> {}

/// `concurrent!` given a context: each arm on its own endpoint and host thread.
#[doc(hidden)]
pub fn concurrent_io<'env, B, E: Send, const N: usize>(
    cx: &'env mut Context,
    receive: &'env [Receive<'env>; N],
    body: B,
) -> Result<(), E>
where
    B: for<'scope> FnOnce(&mut crate::cpu::run::IoArms<'scope, 'env, Io<'env>, E, N>),
{
    crate::cpu::run::spawn(lend(cx, receive), body)
}

/// One endpoint per arm. The lane-tag owner gets the frames the window already gave up.
fn lend<'a, const N: usize>(cx: &'a mut Context, receive: &'a [Receive<'a>; N]) -> [Io<'a>; N] {
    let world = cx.world.as_ref().expect("the communicators were released by `done`");
    let leader = cx.leader.as_ref();
    let (rank, tag) = (cx.rank, cx.lane.tag);
    let workers: &'a [Rank] = &cx.lane.workers;
    #[cfg(feature = "ring")]
    let ring = cx.lane.ring.as_deref();
    #[cfg(feature = "ring")]
    let mut taken = Some(&mut cx.lane.taken);
    core::array::from_fn(|at| {
        let owner = Owner::new(receive, at);
        Io {
            world,
            leader,
            rank,
            lane: tag,
            workers,
            #[cfg(feature = "ring")]
            ring,
            #[cfg(feature = "ring")]
            taken: if owner.owns(tag) { taken.take() } else { None },
            owner,
            next: 0,
            turn: [0; 2],
            _unshared: PhantomData,
        }
    })
}

impl Io<'_> {
    /// The first frame this arm owns from a peer, its leader or a lane. Each call starts one route
    /// after the one that last delivered, so none starves the others.
    pub fn recv(&mut self, out: &mut [u8]) -> Result<Option<Frame>, Error> {
        if !self.owner.receives() {
            return Err(Error::Invalid(Invalid::NotReceiving));
        }
        let me = Participant::Worker(self.rank);
        for k in 0..3 {
            let route = (self.next + k) % 3;
            let frame = match route {
                0 => super::p2p::peer(super::p2p::take(self.world, me, self.owner, &mut self.turn[0], out)?)?,
                1 => match self.leader {
                    Some(leader) => super::p2p::take(leader, me, self.owner, &mut self.turn[1], out)?
                        .map(|(_, tag, len)| Frame::new(None, tag, len)),
                    None => None,
                },
                _ => self.lane_frame(out)?,
            };
            if frame.is_some() {
                self.next = route + 1;
                return Ok(frame);
            }
        }
        Ok(None)
    }

    /// A buffered send leaves no caller storage outstanding.
    pub fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }

    #[cfg(feature = "ring")]
    fn lane_frame(&mut self, out: &mut [u8]) -> Result<Option<Frame>, Error> {
        match (self.ring, self.taken.as_deref_mut()) {
            (Some(ring), Some(taken)) => lane_frame(ring, taken, self.workers, self.lane, self.rank, out),
            _ => Ok(None),
        }
    }

    /// Lanes are ordinary frames here, which the peer route already receives.
    #[cfg(not(feature = "ring"))]
    fn lane_frame(&mut self, _out: &mut [u8]) -> Result<Option<Frame>, Error> {
        Ok(None)
    }
}

/// Ask a communicator to return errors rather than aborting on them.
///
/// The default handler is `MPI_ERRORS_ARE_FATAL`, which terminates the job before any return code
/// exists. A backend that cannot see an error cannot report it, so this is the difference between
/// a contract that permits a failure record and one that does not.
pub(crate) fn return_errors(comm: &SimpleCommunicator) {
    // SAFETY: the handle is live and the handler is a predefined one.
    unsafe { mpi::ffi::MPI_Comm_set_errhandler(comm.as_raw(), mpi::ffi::RSMPI_ERRORS_RETURN) };
}

/// The world ranks that share a node, by lowest rank in each group. Gathered inside `init`, while
/// every rank is still in a collective: a cohort may depend on the answer, so it cannot be lazy.
fn groups(world: &SimpleCommunicator) -> Vec<Rank> {
    use mpi::collective::CommunicatorCollectives;
    let shared = world.split_shared(0);
    let mut peers = vec![0; shared.size() as usize];
    shared.all_gather_into(&world.rank(), &mut peers[..]);
    let leader = peers
        .iter()
        .copied()
        .min()
        .expect("a shared communicator holds its caller");

    let mut leaders = vec![0i32; world.size() as usize];
    world.all_gather_into(&leader, &mut leaders[..]);
    // MPI ranks are non-negative.
    leaders
        .into_iter()
        .map(|leader| Rank::from_index(leader as u32))
        .collect()
}

/// A refusal `init` or `Leader::open` reports, as observed by whoever the process is so far.
pub(crate) fn refused(
    participant: Participant,
    operation: &'static str,
    fault: BackendFault,
) -> Failure {
    Failure {
        participant,
        operation,
        kind: FailureKind::Backend(fault),
    }
}

/// The one place the launch's own numbering enters this backend. MPI reports a rank as a
/// non-negative `int`, so the narrowing to `u32` is exact; `Launch` keeps it from ever being read
/// as a contract [`Rank`].
fn launch(rank: i32) -> Launch {
    Launch::new(rank as u32)
}

/// Enter MPI with the threading and attached buffer every participant and leader needs.
pub(crate) fn enter(
    env: &Environment,
    operation: &'static str,
) -> Result<(Universe, SimpleCommunicator, Launch, u32), Failure> {
    // Before `MPI_Init` a process has no launch rank, so only this refusal is unattributed.
    let (mut universe, threading) = mpi::initialize_with_threading(mpi::Threading::Multiple)
        .ok_or(refused(Participant::Entering(None), operation, BackendFault::Transport))?;
    let job = universe.world();
    let job_rank = launch(job.rank());
    let me = Participant::Entering(Some(job_rank));
    if env.bsend_bytes < BSEND_MIN {
        return Err(refused(me, operation, BackendFault::Storage));
    }
    if threading != mpi::Threading::Multiple {
        // A rank that cannot be called from two threads is not a rank this runtime can run, and
        // finding that out here is the difference between a refusal and a data race.
        return Err(refused(me, operation, BackendFault::Transport));
    }
    universe.set_buffer_size(env.bsend_bytes);
    let job_size = u32::try_from(job.size())
        .map_err(|_| refused(me, operation, BackendFault::Invalid(Invalid::Unrepresentable)))?;
    // Return errors instead of aborting: with MPI's default handler a single failing call kills the
    // process, so no failure could reach the caller as a record — and a buffered send that finds
    // its buffer full is exactly such a call.
    return_errors(&job);
    Ok((universe, job, job_rank, job_size))
}

/// The colour of a rank that is neither a worker nor a leader. It takes part in the split so it
/// cannot hang the ranks that are, and is refused immediately afterwards; the number itself is
/// never read.
const NEITHER: i32 = i32::MAX;

/// Initialise MPI once and return this participant's state.
///
/// The split comes before any collective that touches sharing domains, so a leader that shares a
/// node with workers is never inside one of their domains. Reordering those steps does not produce
/// a wrong number, it produces a hang inside `MPI_Comm_split_type`.
pub fn init(env: Environment, deployment: Deployment<'_>) -> Result<Context, Failure> {
    let (universe, job, job_rank, job_size) = enter(&env, "init")?;

    // The contract's participant set is the workers, not the job.
    //
    // Nothing below the split may return: the split is collective over the whole job, so a rank
    // that left early hangs every rank that stayed. Faults are noted and raised after it.
    let worker = deployment.contract(job_rank);
    let mut fault = None;
    // One worker communicator is the entire contract world. The leader's bridge subgroup is
    // split from it later; splitting the job per leader here would also shrink `size` and `hosts`.
    let pair = worker.and_then(|at| deployment.leader_of(at))
        .filter(|leader| leader.get() < job_size)
        .and_then(|leader| deployment.leader_index(leader));
    if worker.is_some() && deployment.leaders().is_some() && pair.is_none() {
        fault = Some(Invalid::RankOutsideJob);
    }
    let color = if worker.is_some() { 0 } else {
        // Leaders enter this same split from `Leader::open`; a caller entering through the wrong
        // door must participate before reporting its fault.
        fault = Some(Invalid::RankOutsideJob);
        deployment.leader_index(job_rank).map_or(NEITHER, |index| index as i32 + 1)
    }; 
    let key = worker.map_or(job.rank(), |at| at.get() as i32);
    let world = job
        .split_by_color_with_key(Color::with_value(color), key)
        .ok_or(refused(Participant::Entering(Some(job_rank)), "init", BackendFault::Transport))?;
    if let Some(why) = fault {
        return Err(refused(Participant::Entering(Some(job_rank)), "init", BackendFault::Invalid(why)));
    }

    let unrepresentable = BackendFault::Invalid(Invalid::Unrepresentable);
    let rank = u32::try_from(world.rank())
        .map(Rank::from_index)
        .map_err(|_| refused(Participant::Entering(Some(job_rank)), "init", unrepresentable))?;
    let size = u32::try_from(world.size()).map_err(|_| refused(Participant::Worker(rank), "init", unrepresentable))?;
    return_errors(&world);
    // Domains are discovered inside the participant set, after the split. The leader is not in this
    // communicator, so it cannot be inside a worker's sharing domain.
    let hosts = groups(&world);

    // Every worker splits the same worker world, including workers assigned to other leaders.
    // Only members of each resulting subgroup build its bridge, against the job communicator.
    let leader_group = match pair {
        None => None,
        Some(index) => Some(world.split_by_color_with_key(
            Color::with_value(i32::try_from(index).map_err(|_| refused(Participant::Worker(rank), "init", unrepresentable))?),
            world.rank(),
        ).ok_or(refused(Participant::Worker(rank), "init", BackendFault::Transport))?),
    };
    let leader_route = match (&leader_group, pair) {
        (Some(group), Some(index)) => {
            let leader = deployment.leader_of(rank).ok_or(refused(
                Participant::Worker(rank), "init", BackendFault::Invalid(Invalid::NoLeader),
            ))?;
            Some(super::leader::bridge(group, &job, leader, index)
                .ok_or(refused(Participant::Worker(rank), "init", BackendFault::Transport))?)
        }
        _ => None,
    }; 

    // The cohort is the sharing domain: the workers whose `hosts` entry is this rank's.
    let color = i32::try_from(hosts[rank.get() as usize].get()).map_err(|_| refused(Participant::Worker(rank), "init", unrepresentable))?;
    let together = world
        .split_by_color_with_key(Color::with_value(color), world.rank())
        .ok_or(refused(Participant::Worker(rank), "init", BackendFault::Transport))?;

    return_errors(&together);

    Ok(Context {
        _universe: ManuallyDrop::new(universe),
        world: Some(world),
        together: Some(together),
        rank,
        size,
        hosts,
        leader: leader_route,
        leader_group,
        lane: Lane::none(),
        _unshared: PhantomData,
    })
}

/// Release this participant's MPI resources and report the outcome.
///
/// The order matters and is the whole of this function. The buffered-send storage is detached
/// first: detachment waits for every accepted buffered send to leave it, and those sends still need
/// their communicators while that happens. The communicators are then released while MPI is live,
/// because freeing one after `MPI_Finalize` aborts the job instead of reporting a failure.
pub fn done<A>(cx: &mut Context, outcome: Result<(), Failure<A>>) -> Result<(), Failure<A>> {
    cx._universe.detach_buffer();
    // The bridge first, because disconnecting it is collective over both groups and the workers'
    // ends are released at this same point in their own `done`.
    drop(cx.leader.take());
    drop(cx.leader_group.take());
    drop(cx.together.take());
    drop(cx.world.take());
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::Write::flush(&mut std::io::stderr());
    // SAFETY: every MPI call this process makes has returned, and the communicators are released.
    unsafe { mpi::ffi::MPI_Finalize() };
    outcome
}

pub fn rank(cx: &Context) -> Rank {
    cx.rank
}

pub fn size(cx: &Context) -> u32 {
    cx.size
}

pub fn hosts(cx: &Context) -> &[Rank] {
    &cx.hosts
}

/// A collective over the entered workers, so a caller coordinates a phase without a frame.
///
/// It is `together()`, the worker-only communicator, and never the world or a leader bridge: a
/// barrier that a leader could enter would not be a worker phase boundary. It consumes no message
/// and no lane, so it cannot take a frame belonging to another claim.
pub fn barrier(cx: &mut Context) {
    cx.together().barrier();
}

/// Ring slots per destination element. The declaration's `affected` is read against this to size
/// a pair's depth, and it is the same factor in every ring transport because the declaration is
/// the contract's rather than a transport's.
#[cfg(feature = "ring")]
pub const FACTOR: usize = 4;

/// The window's lane table: positions among the workers, the depth the declaration implies, and
/// the slot width.
///
/// Positions and not world ranks, because a window is indexed by place in the worker list. Sorted
/// so every member opens the same window without depending on the order the declaration happened
/// to arrive in.
#[cfg(feature = "ring")]
pub(crate) fn window(
    workers: &[Rank],
    edges: &[Edge],
    bytes: usize,
) -> Result<Vec<(i32, i32, usize, usize)>, Error> {
    let position = |rank: Rank| {
        let at = workers
            .iter()
            .position(|&worker| worker == rank)
            .ok_or(Error::Invalid(Invalid::EdgeOutsideWorkers))?;
        i32::try_from(at).map_err(|_| Error::Invalid(Invalid::Unrepresentable))
    };
    let mut lanes = Vec::with_capacity(edges.len());
    for edge in edges {
        let depth = usize::try_from(edge.affected().get())
            .ok()
            .and_then(|affected| affected.checked_mul(FACTOR))
            .ok_or(Error::Invalid(Invalid::Unrepresentable))?;
        lanes.push((
            position(edge.source())?,
            position(edge.destination())?,
            depth,
            bytes,
        ));
    }
    lanes.sort_unstable();
    Ok(lanes)
}

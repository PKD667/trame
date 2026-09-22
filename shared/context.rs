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
use std::mem::ManuallyDrop;
#[cfg(feature = "ring")]
use std::sync::Arc;

use mpi::environment::Universe;
use mpi::point_to_point::Message;
use mpi::raw::traits::AsRaw;
use mpi::topology::{Color, Communicator, InterCommunicator, SimpleCommunicator};

#[cfg(feature = "ring")]
use mpi_rma::Ring;

#[cfg(feature = "ring")]
use crate::contract::Frame;
use crate::contract::{
    BackendFault, Deployment, Edge, Error, Failure, FailureKind, FrameBytes, Invalid, Rank, Tag,
};

/// Megabytes attached for buffered sends, when the entry says nothing.
const BSEND_MB: usize = 128;

/// The longest frame: half the default attached buffer. Every send is buffered, so a frame must fit
/// the attached buffer beside MPI's per-message bookkeeping, whose size MPI does not state; the
/// other half is that margin. `init` and `Leader::open` refuse a smaller buffer.
pub const MAX_FRAME: FrameBytes = FrameBytes::new((BSEND_MB << 20) as u32 / 2);

/// The smallest attached buffer an entry may state.
const BSEND_MIN: usize = 2 * MAX_FRAME.get() as usize;

/// What the entry supplies.
///
/// MPI discovers its own world, so there is nothing here that a process could not find out,
/// except the one number MPI will not report: the attached buffer's size, which is where every
/// accepted frame waits.
pub struct Environment {
    /// Bytes attached for buffered sends; at least twice `MAX_FRAME`.
    pub bsend_bytes: usize,
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
    /// The leader route's own hold slot. One per route, because a frame the leader route matched
    /// and could not deliver must not be surrendered because the message route refused one.
    leader_held: Option<(Message, FrameBytes)>,
    lane: Lane,
    /// A message that was matched and did not fit the caller's buffer.
    ///
    /// MPI has no un-probe: a matched message is received or cancelled, and cancelling is not
    /// guaranteed. Holding it is what keeps a refusal non-consuming — the frame the caller was
    /// told to grow for is the frame the next call hands over — and it is why the refusal can be
    /// reported at all, since a plain probe would have to be followed by a receive that a
    /// concurrent sender may have already invalidated.
    held: Option<(Message, FrameBytes)>,
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
        if self.lane.workers.is_empty() {
            return Err(Error::Invalid(Invalid::LaneNotConfigured));
        }
        Ok(self.lane.tag)
    }

    /// This participant's position in the lane table, which is what the window is indexed by.
    ///
    /// `Rank` is a world rank and a window is indexed by position among the workers, so the two
    /// are not the same number and the mapping is not the identity.
    #[cfg(feature = "ring")]
    pub(crate) fn lane_index(&self, rank: Rank) -> Result<i32, Error> {
        let at = self
            .lane
            .workers
            .iter()
            .position(|&worker| worker == rank)
            .ok_or(Error::Invalid(Invalid::NoLane))?;
        i32::try_from(at).map_err(|_| Error::Invalid(Invalid::Unrepresentable))
    }

    /// The world rank a lane index names.
    #[cfg(feature = "ring")]
    fn lane_peer(&self, index: i32) -> Result<Rank, Error> {
        usize::try_from(index)
            .ok()
            .and_then(|at| self.lane.workers.get(at).copied())
            .ok_or(Error::Invalid(Invalid::RankOutsideJob))
    }

    /// One attempt at the next lane frame: the oldest frame already taken from the window, or, when
    /// none is, one poll of it. A frame that does not fit stays first in line.
    #[cfg(feature = "ring")]
    pub(crate) fn next_lane_frame(&mut self, out: &mut [u8]) -> Result<Option<Frame>, Error> {
        if self.lane.taken.is_empty() {
            let Some(ring) = self.lane.ring.clone() else {
                return Ok(None);
            };
            let messages = ring.poll().map_err(|_| self.failure("poll"))?;
            for message in messages {
                let source = self.lane_peer(message.origin)?;
                self.lane.taken.push_back((source, message.data));
            }
        }
        let Some((source, data)) = self.lane.taken.front() else {
            return Ok(None);
        };
        let len = FrameBytes::try_from(data.len()).map_err(Error::Invalid)?;
        if data.len() > out.len() {
            return Err(Error::TooSmall { needed: len });
        }
        out[..data.len()].copy_from_slice(data);
        let frame = Frame::new(*source, self.lane.tag, len);
        self.lane.taken.pop_front();
        Ok(Some(frame))
    }

    /// This route's communicator and hold slot, borrowed at once.
    ///
    /// A receive needs both — the communicator to match on, the slot to keep a frame it matched
    /// and could not deliver — and both are fields of this record. Returning them together is what
    /// lets one generic receive serve this route and the leader's: `split` is the borrow that makes
    /// a single rule reachable from two communicators, rather than a second copy of the rule.
    pub(crate) fn split(&mut self) -> (&SimpleCommunicator, &mut Option<(Message, FrameBytes)>) {
        (
            self.world
                .as_ref()
                .expect("the communicators were released by `done`"),
            &mut self.held,
        )
    }

    /// The leader route's communicator and hold slot, borrowed at once, or `None` when the
    /// deployment named no leader. The same shape as [`split`](Self::split) for the same reason.
    pub(crate) fn leader_split(
        &mut self,
    ) -> Option<(&InterCommunicator, &mut Option<(Message, FrameBytes)>)> {
        let leader = self.leader.as_ref()?;
        Some((leader, &mut self.leader_held))
    }

    pub(crate) fn failure(&self, operation: &'static str) -> Error {
        Error::Failed(Failure {
            participant: self.rank,
            operation,
            kind: FailureKind::Backend(BackendFault::Transport),
        })
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

/// A refusal `init` or `Leader::open` reports before it has a rank of its own to report it as.
pub(crate) fn refused(
    participant: Rank,
    operation: &'static str,
    fault: BackendFault,
) -> Failure {
    Failure {
        participant,
        operation,
        kind: FailureKind::Backend(fault),
    }
}

/// Enter MPI with the threading and attached buffer every participant and leader needs.
pub(crate) fn enter(
    env: &Environment,
    operation: &'static str,
) -> Result<(Universe, SimpleCommunicator, Rank, u32), Failure> {
    let nobody = Rank::from_index(0);
    if env.bsend_bytes < BSEND_MIN {
        return Err(refused(nobody, operation, BackendFault::Storage));
    }
    let (mut universe, threading) = mpi::initialize_with_threading(mpi::Threading::Multiple)
        .ok_or(refused(nobody, operation, BackendFault::Transport))?;
    if threading != mpi::Threading::Multiple {
        // A rank that cannot be called from two threads is not a rank this runtime can run, and
        // finding that out here is the difference between a refusal and a data race.
        return Err(refused(nobody, operation, BackendFault::Transport));
    }
    universe.set_buffer_size(env.bsend_bytes);
    let job = universe.world();
    let unrepresentable = BackendFault::Invalid(Invalid::Unrepresentable);
    let job_rank = u32::try_from(job.rank())
        .map(Rank::from_index)
        .map_err(|_| refused(nobody, operation, unrepresentable))?;
    let job_size =
        u32::try_from(job.size()).map_err(|_| refused(job_rank, operation, unrepresentable))?;
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
pub fn init(
    env: Environment,
    deployment: Deployment<'_>,
    rule: fn(Rank, &[Rank]) -> u32,
) -> Result<Context, Failure> {
    let (universe, job, job_rank, job_size) = enter(&env, "init")?;

    // The contract's participant set is the workers, not the job.
    //
    // Nothing below the split may return: the split is collective over the whole job, so a rank
    // that left early hangs every rank that stayed. Faults are noted and raised after it.
    let worker = deployment.contract(job_rank);
    let mut fault = None;
    // The colour pairs a worker with the leader that serves it: a worker's is even, its leader's is
    // the odd number above it, and both ends derive the index from the declaration. One computation
    // rather than two, which is what stops two leaders bridging to each other's workers.
    let (color, pair) = match (worker, deployment.leaders()) {
        (Some(_), None) => (0, None),
        (Some(at), Some(_)) => match deployment
            .leader_of(at)
            .filter(|leader| leader.get() < job_size)
            .and_then(|leader| deployment.leader_index(leader))
        {
            Some(index) => ((index * 2) as i32, Some(index)),
            None => {
                fault = Some(Invalid::RankOutsideJob);
                (NEITHER, None)
            }
        },
        (None, _) => {
            // A leader or a rank the deployment does not name. Either way it entered through the
            // wrong door: a leader's entry is `Leader::open`, which builds the other half of the
            // bridge this rank would leave. It takes part so the split completes.
            fault = Some(Invalid::RankOutsideJob);
            match deployment.leader_index(job_rank) {
                Some(index) => ((index * 2 + 1) as i32, None),
                None => (NEITHER, None),
            }
        }
    };
    let key = worker.map_or(job.rank(), |at| at.get() as i32);
    let world = job
        .split_by_color_with_key(Color::with_value(color), key)
        .ok_or(refused(job_rank, "init", BackendFault::Transport))?;
    if let Some(why) = fault {
        return Err(refused(job_rank, "init", BackendFault::Invalid(why)));
    }

    let unrepresentable = BackendFault::Invalid(Invalid::Unrepresentable);
    let rank = u32::try_from(world.rank())
        .map(Rank::from_index)
        .map_err(|_| refused(job_rank, "init", unrepresentable))?;
    let size = u32::try_from(world.size()).map_err(|_| refused(rank, "init", unrepresentable))?;
    return_errors(&world);
    // Domains are discovered inside the participant set, after the split. The leader is not in this
    // communicator, so it cannot be inside a worker's sharing domain.
    let hosts = groups(&world);

    // The bridge, when the declaration names a leader for this worker. Both ends run this
    // collective, from two different entries: the workers from here and each leader from
    // `Leader::open`.
    let leader_route = match pair {
        None => None,
        Some(index) => {
            let leader = deployment.leader_of(rank).ok_or(refused(
                rank,
                "init",
                BackendFault::Invalid(Invalid::NoLeader),
            ))?;
            Some(
                super::leader::bridge(&world, &job, leader, index)
                    .ok_or(refused(rank, "init", BackendFault::Transport))?,
            )
        }
    };

    // The rule returns a colour, so two ranks that agree run collectives together and the caller
    // names no list.
    let color = i32::try_from(rule(rank, &hosts)).map_err(|_| refused(rank, "init", unrepresentable))?;
    let together = world
        .split_by_color_with_key(Color::with_value(color), world.rank())
        .ok_or(refused(rank, "init", BackendFault::Transport))?;

    return_errors(&together);

    Ok(Context {
        _universe: ManuallyDrop::new(universe),
        world: Some(world),
        together: Some(together),
        rank,
        size,
        hosts,
        leader: leader_route,
        leader_held: None,
        lane: Lane::none(),
        held: None,
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

pub fn align() -> usize {
    crate::partition::host_align()
}

/// The checks `reshape` makes whatever the lane transport is: ascending unique workers, edges
/// ordered and duplicate-free, both endpoints workers, and a frame the backend can carry. Whether
/// the window can hold the implied depth is each transport's own, because only it knows what it
/// built.
pub(crate) fn validate(workers: &[Rank], edges: &[Edge], bytes: FrameBytes) -> Result<(), Error> {
    if workers.windows(2).any(|w| w[0] >= w[1]) {
        return Err(Error::Invalid(Invalid::UnorderedWorkers));
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
    Ok(())
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
    bytes: FrameBytes,
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
            bytes.get() as usize,
        ));
    }
    lanes.sort_unstable();
    Ok(lanes)
}

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

use std::mem::ManuallyDrop;
#[cfg(feature = "ring")]
use std::sync::Arc;

use mpi::environment::Universe;
use mpi::point_to_point::Message;
use mpi::raw::traits::AsRaw;
use mpi::topology::{Color, Communicator, InterCommunicator, SimpleCommunicator};

#[cfg(feature = "ring")]
use mpi_rma::Ring;

use crate::Rank;
use crate::contract::{Deployment, Edge, Error, Failure, Reading, Tag};

/// Megabytes attached for buffered sends, when the entry says nothing.
pub const BSEND_MB: usize = 128;

/// What the entry supplies.
///
/// MPI discovers its own world — the declarations allow a host binding to do that — so there is
/// nothing here that a process could not find out, except the one number MPI will not report: the
/// attached buffer's size. That number is an input because it decides which frames `Wait::Poll` can
/// accept, and a bound nobody stated is a bound nobody can act on.
pub struct Environment {
    /// Bytes attached for buffered sends. `Wait::Poll` refuses a frame larger than this.
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
    cohort: Vec<Rank>,
    bsend_bytes: usize,
    /// The worker's end of the bridge to its leader, when the deployment named one.
    ///
    /// A different communicator of a different *kind*, which is why it is a second field rather
    /// than a rank the message route could also have used: an inter-communicator addresses a remote
    /// group, and folding it into the message route would be treating two kinds of thing as one.
    leader: Option<InterCommunicator>,
    /// The leader route's own hold slot. One per route, because a frame the leader route matched
    /// and could not deliver must not be surrendered because the message route refused one.
    leader_held: Option<Message>,
    lane: Lane,
    /// A message that was matched and did not fit the caller's buffer.
    ///
    /// MPI has no un-probe: a matched message is received or cancelled, and cancelling is not
    /// guaranteed. Holding it is what keeps a refusal non-consuming — the frame the caller was
    /// told to grow for is the frame the next call hands over — and it is why the refusal can be
    /// reported at all, since a plain probe would have to be followed by a receive that a
    /// concurrent sender may have already invalidated.
    held: Option<Message>,
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

    pub(crate) fn bsend_bytes(&self) -> usize {
        self.bsend_bytes
    }

    /// Take the lane table. Only the tag and the worker list: a backend whose lanes are ordinary
    /// frames has nothing else, and one whose lanes are not calls [`set_window`](Self::set_window).
    pub(crate) fn set_lane(&mut self, tag: Tag, workers: Vec<Rank>) {
        self.lane = Lane {
            tag,
            workers,
            #[cfg(feature = "ring")]
            ring: None,
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
        };
    }

    /// The window the current load opened, if any. Ring builds only.
    #[cfg(feature = "ring")]
    pub(crate) fn ring(&self) -> Option<Arc<Ring>> {
        self.lane.ring.clone()
    }

    pub(crate) fn clear_lane(&mut self) {
        self.set_lane(0, Vec::new());
    }

    /// The lane's tag, or `Invalid` when no load declared one.
    pub(crate) fn lane_tag(&self) -> Result<Tag, Error> {
        if self.lane.workers.is_empty() {
            return Err(Error::Invalid { code: 1 });
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
            .ok_or(Error::Invalid { code: 2 })?;
        i32::try_from(at).map_err(|_| Error::Invalid { code: 3 })
    }

    /// The world rank a lane index names.
    #[cfg(feature = "ring")]
    pub(crate) fn lane_peer(&self, index: i32) -> Result<Rank, Error> {
        let at = usize::try_from(index)
            .ok()
            .and_then(|at| self.lane.workers.get(at).copied())
            .ok_or(Error::Invalid { code: 4 })?;
        Ok(at)
    }

    /// Every frame waiting in the window, with its origin as a world rank.
    #[cfg(feature = "ring")]
    pub(crate) fn drain(&self) -> Result<Vec<(Rank, Vec<u8>)>, Error> {
        let Some(ring) = self.lane.ring.as_ref() else {
            return Ok(Vec::new());
        };
        let messages = ring.poll().map_err(|_| self.failure("poll"))?;
        messages
            .into_iter()
            .map(|message| Ok((self.lane_peer(message.origin)?, message.data)))
            .collect()
    }

    /// This route's communicator and hold slot, borrowed at once.
    ///
    /// A receive needs both — the communicator to match on, the slot to keep a frame it matched
    /// and could not deliver — and both are fields of this record. Returning them together is what
    /// lets one generic receive serve this route and the leader's: `split` is the borrow that makes
    /// a single rule reachable from two communicators, rather than a second copy of the rule.
    pub(crate) fn split(&mut self) -> (&SimpleCommunicator, &mut Option<Message>) {
        (
            self.world
                .as_ref()
                .expect("the communicators were released by `done`"),
            &mut self.held,
        )
    }

    /// The leader route's communicator and hold slot, borrowed at once, or `None` when the
    /// deployment named no leader. The same shape as [`split`](Self::split) for the same reason.
    pub(crate) fn leader_split(&mut self) -> Option<(&InterCommunicator, &mut Option<Message>)> {
        let leader = self.leader.as_ref()?;
        Some((leader, &mut self.leader_held))
    }

    pub(crate) fn failure(&self, operation: &'static str) -> Error {
        Error::Failed(Failure {
            participant: self.rank,
            operation,
            code: 0,
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
    let leader = peers.iter().copied().min().unwrap_or(world.rank());

    let mut leaders = vec![0; world.size() as usize];
    world.all_gather_into(&leader, &mut leaders[..]);
    leaders
        .into_iter()
        .map(|leader| u32::try_from(leader).unwrap_or(0))
        .collect()
}

/// The colour of a rank that is neither a worker nor a leader. It takes part in the split so it
/// cannot hang the ranks that are, and is refused immediately afterwards; the number itself is
/// never read.
const NEITHER: i32 = i32::MAX;

/// Initialise MPI once and return this participant's state.
///
/// Idempotent as far as MPI allows: a second call after `MPI_Finalize` re-initialises, which is the
/// one thing a process cannot do twice. The entry rules require the entry to establish the world
/// before any peer operation, and this is that point for an MPI build.
///
/// The deployment is checked, then acted on, and the order of the two steps after it is fixed: the
/// split comes before any collective that touches sharing domains, so a leader that shares a node
/// with workers is never inside one of their domains. Reordering those steps does not produce a
/// wrong number, it produces a hang inside `MPI_Comm_split_type`, which is why the reason is
/// written here rather than left to be re-derived.
pub fn init(
    env: Environment,
    deployment: Deployment<'_>,
    rule: fn(Rank, &[Rank]) -> u32,
) -> Result<Context, Failure> {
    if deployment.refuse().is_some() {
        return Err(Failure {
            participant: 0,
            operation: "init",
            code: 7,
        });
    }
    let (mut universe, threading) = mpi::initialize_with_threading(mpi::Threading::Multiple)
        .ok_or(Failure {
            participant: 0,
            operation: "init",
            code: 1,
        })?;
    if threading != mpi::Threading::Multiple {
        // A rank that cannot be called from two threads is not a rank this runtime can run, and
        // finding that out here is the difference between a refusal and a data race.
        return Err(Failure {
            participant: 0,
            operation: "init",
            code: 2,
        });
    }
    universe.set_buffer_size(env.bsend_bytes);

    let job = universe.world();
    let job_rank = u32::try_from(job.rank()).map_err(|_| Failure {
        participant: 0,
        operation: "init",
        code: 3,
    })?;
    let job_size = u32::try_from(job.size()).map_err(|_| Failure {
        participant: 0,
        operation: "init",
        code: 4,
    })?;
    // Return errors instead of aborting. This is the entry rule before it is anything else's:
    // with MPI's default handler a single failing call kills the process, so no failure can reach
    // the caller as a record — and a buffered send that finds its buffer full is exactly such a
    // call, which is why capacity pressure was previously unreportable rather than unreported.
    return_errors(&job);

    // The contract's participant set is the workers, not the job. A deployment that names them
    // gets a communicator holding exactly those, and one that names none is the whole job — which
    // is the widening the type promises, so both readings come out of this single expression.
    //
    // Nothing below the split may return: the split is collective over the whole job, so a rank
    // that left early hangs every rank that stayed. Faults are noted and raised after it.
    let worker = deployment.contract(job_rank).or_else(|| {
        // Empty means every rank is a worker, and then a contract rank is the job's own rank.
        deployment.workers.is_empty().then_some(job_rank)
    });
    let mut fault = 0;
    // The colour pairs a worker with the leader that serves it: a worker's is even, its leader's is
    // the odd number above it, and both ends derive the index from the declaration. One computation
    // rather than two, which is what stops two leaders bridging to each other's workers.
    let (color, pair) = if deployment.workers.is_empty() {
        (0, None)
    } else {
        match worker {
            Some(at) => match deployment.leader_of(at).filter(|&leader| leader < job_size) {
                Some(leader) => match deployment.leader_index(leader) {
                    Some(index) => ((index * 2) as i32, Some(index)),
                    None => {
                        fault = 11;
                        (NEITHER, None)
                    }
                },
                None => {
                    fault = 9;
                    (NEITHER, None)
                }
            },
            None => match deployment.leader_index(job_rank) {
                Some(index) => ((index * 2 + 1) as i32, Some(index)),
                // Neither a worker nor a leader. It takes part so the split completes, and is
                // refused once it has.
                None => {
                    fault = 8;
                    (NEITHER, None)
                }
            },
        }
    };
    let key = worker.map(|at| at as i32).unwrap_or(job.rank());
    let world = job
        .split_by_color_with_key(Color::with_value(color), key)
        .ok_or(Failure {
            participant: job_rank,
            operation: "init",
            code: 6,
        })?;
    if fault != 0 || color % 2 != 0 {
        // A process that is not a worker entered through the wrong door; its entry is
        // `Leader::open`, which is what creates the other half of the bridge this rank left.
        return Err(Failure {
            participant: job_rank,
            operation: "init",
            code: if fault != 0 { fault } else { 8 },
        });
    }

    let rank = u32::try_from(world.rank()).map_err(|_| Failure {
        participant: 0,
        operation: "init",
        code: 3,
    })?;
    let size = u32::try_from(world.size()).map_err(|_| Failure {
        participant: 0,
        operation: "init",
        code: 4,
    })?;
    return_errors(&world);
    // Domains are discovered inside the participant set, after the split. The leader is not in this
    // communicator, so it cannot be inside a worker's sharing domain — and that is why `hosts` is a
    // statement about memory and no longer evidence of leadership.
    let hosts = groups(&world);

    // The bridge, when the declaration names a leader for this worker. Both ends run this
    // collective, from two different entries: the workers from here and each leader from
    // `Leader::open`. Each names the *other* group's leader, which is the asymmetry rsmpi has no
    // wrapper for.
    let leader_route = match pair {
        None => None,
        Some(index) => {
            let leader = deployment.leader_of(rank).ok_or(Failure {
                participant: rank,
                operation: "init",
                code: 11,
            })?;
            Some(
                super::leader::bridge(&world, &job, leader, index).ok_or(Failure {
                    participant: rank,
                    operation: "init",
                    code: 10,
                })?,
            )
        }
    };

    // The rule returns a colour, so two ranks that agree run collectives together and the caller
    // names no list. Everyone gathers everyone's colour because the cohort's *membership* has to
    // be answerable afterwards, and the answer must not depend on who is asked.
    let color = i32::try_from(rule(rank, &hosts)).map_err(|_| Failure {
        participant: rank,
        operation: "init",
        code: 5,
    })?;
    let together = world
        .split_by_color_with_key(Color::with_value(color), world.rank())
        .ok_or(Failure {
            participant: rank,
            operation: "init",
            code: 6,
        })?;

    // Every rank gathers every rank's colour: the cohort's *membership* has to be answerable
    // afterwards, and the answer must not depend on which member is asked. One collective, so it
    // cannot race a receive loop the way a communicator built per load would.
    use mpi::collective::CommunicatorCollectives;
    let mut colors = vec![0i32; size as usize];
    world.all_gather_into(&color, &mut colors[..]);
    let cohort: Vec<Rank> = colors
        .iter()
        .enumerate()
        .filter(|&(_, &colour)| colour == color)
        .map(|(at, _)| at as Rank)
        .collect();

    return_errors(&together);

    Ok(Context {
        _universe: ManuallyDrop::new(universe),
        world: Some(world),
        together: Some(together),
        rank,
        size,
        hosts,
        cohort,
        bsend_bytes: env.bsend_bytes,
        leader: leader_route,
        leader_held: None,
        lane: Lane {
            tag: 0,
            workers: Vec::new(),
            #[cfg(feature = "ring")]
            ring: None,
        },
        held: None,
    })
}

/// Release this participant's MPI resources and report the outcome.
///
/// It returns rather than exiting: the entry rules make process exit non-portable, and a caller
/// that wants to stop is the caller's business.
///
/// The order matters and is the whole of this function. The buffered-send storage is detached
/// first: detachment waits for every accepted buffered send to leave it, and those sends still need
/// their communicators while that happens. The communicators are then released while MPI is live,
/// because freeing one after `MPI_Finalize` is disallowed and aborts the job instead of reporting a
/// failure. The universe is never dropped — it is held in `ManuallyDrop` — so finalising remains
/// this call's explicit act rather than a destructor's later side effect.
pub fn done(cx: &mut Context, outcome: Result<(), Failure>) -> Result<(), Failure> {
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

#[inline]
pub fn rank(cx: &Context) -> Rank {
    cx.rank
}

#[inline]
pub fn size(cx: &Context) -> u32 {
    cx.size
}

#[inline]
pub fn hosts(cx: &Context) -> &[Rank] {
    &cx.hosts
}

#[inline]
pub fn cohort(cx: &Context) -> &[Rank] {
    &cx.cohort
}

/// The process's monotonic clock. An MPI rank is a host thread, so the host's clock is its clock.
pub fn reading(_cx: &Context) -> Result<Reading, Error> {
    Ok(crate::cpu::clock::reading())
}

/// This participant's share of a segment, by the shared pure rule.
pub fn slice(cx: &Context, rank: Rank, total: usize) -> (usize, usize) {
    crate::partition::slice_of(
        hosts(cx),
        cohort(cx),
        rank,
        total,
        crate::partition::host_align(),
    )
}

/// The checks `reshape` makes whatever the lane transport is.
///
/// Shared by all three MPI backends because the declaration is the contract's, not a transport's:
/// ascending unique workers, edges ordered and duplicate-free, both endpoints workers, and a frame
/// the backend can carry. The launch-specific check — whether the window can hold the implied
/// depth — is each transport's own, because only it knows what it built.
pub(crate) fn validate(
    workers: &[Rank],
    edges: &[Edge],
    bytes: u32,
    max_frame: u32,
) -> Result<(), Error> {
    if workers.windows(2).any(|w| w[0] >= w[1]) {
        return Err(Error::Invalid { code: 7 });
    }
    if edges
        .windows(2)
        .any(|w| (w[0].source, w[0].destination) >= (w[1].source, w[1].destination))
    {
        return Err(Error::Invalid { code: 8 });
    }
    for edge in edges {
        if !workers.contains(&edge.source) || !workers.contains(&edge.destination) {
            return Err(Error::Invalid { code: 9 });
        }
    }
    if bytes > max_frame {
        return Err(Error::TooLarge { limit: max_frame });
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
/// Positions and not world ranks, because a window is indexed by place in the worker list; the
/// mapping back is [`Context::lane_peer`]. Sorted so every member opens the same window without
/// depending on the order the declaration happened to arrive in, which is the property that lets
/// a collective be built from a value rather than from an iteration.
#[cfg(feature = "ring")]
pub(crate) fn window(
    workers: &[Rank],
    edges: &[Edge],
    bytes: u32,
) -> Result<Vec<(i32, i32, usize, usize)>, Error> {
    let position = |rank: Rank| {
        workers
            .iter()
            .position(|&worker| worker == rank)
            .ok_or(Error::Invalid { code: 10 })
    };
    let mut lanes = Vec::with_capacity(edges.len());
    for edge in edges {
        let source =
            i32::try_from(position(edge.source)?).map_err(|_| Error::Invalid { code: 11 })?;
        let destination =
            i32::try_from(position(edge.destination)?).map_err(|_| Error::Invalid { code: 12 })?;
        let depth = usize::try_from(edge.affected)
            .ok()
            .and_then(|affected| affected.checked_mul(FACTOR))
            .ok_or(Error::Invalid { code: 13 })?;
        lanes.push((source, destination, depth, bytes as usize));
    }
    lanes.sort_unstable();
    Ok(lanes)
}

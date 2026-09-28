// The value types every backend shares.
//
// These are here rather than in each backend because they carry no backend choice: `Frame` is a
// source, a tag and a length wherever it came from, and `Error` is the set of refusals the surface
// promises to distinguish. `Context`, `Environment` and `Shared` are *not* here, because those are
// where a backend's storage, identity and lifetime actually differ.
//
// `Rank` and `Tag` are newtypes with private fields so that a rank cannot be passed where a tag
// was meant, and a backend converts each at its own boundary with a check: a value that does not
// fit is an `Invalid` rather than a truncation, because a truncated rank is a frame delivered to
// the wrong participant.
//
// `Rank` and `Launch` are two identities and never interchangeable. A `Rank` is a contract rank:
// a participant's dense index in the cohort. A `Launch` is a launch rank: the number the transport
// itself assigned the process. The same contract rank is a different launch rank in a different
// job, so a backend converts between them in exactly one place and the types force every other
// use through it.
//
// `Span` is an integer count of nanoseconds and deliberately not `std::time::Duration`: a device
// has no such type, and a duration that carries its own clock is a duration that can be subtracted
// from the wrong one. Only readings sharing a `ClockId` may be compared.

use core::convert::Infallible;
use core::fmt;
use core::num::NonZeroU32;
use core::num::NonZeroU64;

/// A participant's dense index in the cohort, `[0, size)`, or a launch's number for a process in a
/// [`Deployment`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rank(u32);

impl Rank {
    pub const fn from_index(index: u32) -> Self {
        Rank(index)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A process's launch rank: the number the transport assigned it. Distinct from [`Rank`], which
/// is its dense contract rank, and never passed where one is meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Launch(u32);

impl Launch {
    pub const fn new(rank: u32) -> Self {
        Launch(rank)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A frame's tag. One width for every backend, so a program cannot depend on a tag one backend
/// carries and another does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(u16);

impl Tag {
    pub const fn new(tag: u16) -> Self {
        Tag(tag)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A published segment's name, as a worker receives it in an application frame.
///
/// Fixed-size and plain so any frame format can carry it: a revision, so a worker can tell which
/// publication it attached, a length, and a token only the selected backend interprets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    revision: NonZeroU64,
    length: u64,
    token: u64,
}

impl Handle {
    pub const BYTES: usize = 24;

    // Read only by the backends that need it; which ones is not this file's to name.
    #[allow(dead_code)]
    pub(crate) const fn new(revision: NonZeroU64, length: u64, token: u64) -> Self {
        Handle {
            revision,
            length,
            token,
        }
    }

    // Read only by the backends that need it; which ones is not this file's to name.
    #[allow(dead_code)]
    pub(crate) const fn revision(self) -> NonZeroU64 {
        self.revision
    }

    // Read only by the backends that need it; which ones is not this file's to name.
    #[allow(dead_code)]
    pub(crate) const fn length(self) -> u64 {
        self.length
    }

    // Read only by the backends that need it; which ones is not this file's to name.
    #[allow(dead_code)]
    pub(crate) const fn token(self) -> u64 {
        self.token
    }

    /// Revision, length, token, each little-endian.
    pub fn to_bytes(self) -> [u8; Self::BYTES] {
        let mut bytes = [0u8; Self::BYTES];
        bytes[0..8].copy_from_slice(&self.revision.get().to_le_bytes());
        bytes[8..16].copy_from_slice(&self.length.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.token.to_le_bytes());
        bytes
    }

    /// Revision zero is never published, so it is refused as `Unrepresentable`.
    pub fn from_bytes(bytes: [u8; Self::BYTES]) -> Result<Self, Invalid> {
        let revision = NonZeroU64::new(u64::from_le_bytes(
            bytes[0..8].try_into().expect("eight bytes"),
        ))
        .ok_or(Invalid::Unrepresentable)?;
        let length = u64::from_le_bytes(bytes[8..16].try_into().expect("eight bytes"));
        let token = u64::from_le_bytes(bytes[16..24].try_into().expect("eight bytes"));
        Ok(Handle {
            revision,
            length,
            token,
        })
    }
}

/// Which backend this build selected. The discriminants are on disk in every `LOAD` record, so
/// they only ever grow at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Backend {
    Mpi = 0,
    Rma = 1,
    RmaLossy = 2,
    None = 3,
    Nv = 4,
}

impl Backend {
    pub const fn wire_id(self) -> u8 {
        self as u8
    }

    pub const fn name(self) -> &'static str {
        match self {
            Backend::Mpi => "mpi",
            Backend::Rma => "rma",
            Backend::RmaLossy => "rma-lossy",
            Backend::None => "none",
            Backend::Nv => "nv",
        }
    }
}

/// The reading direction of `wire_id`: a log reader decodes records any backend wrote.
impl TryFrom<u8> for Backend {
    type Error = Invalid;

    fn try_from(id: u8) -> Result<Self, Invalid> {
        match id {
            0 => Ok(Backend::Mpi),
            1 => Ok(Backend::Rma),
            2 => Ok(Backend::RmaLossy),
            3 => Ok(Backend::None),
            4 => Ok(Backend::Nv),
            _ => Err(Invalid::Unrepresentable),
        }
    }
}

/// What the launch says about who is who.
///
/// A leader is not a participant, so nothing in a participant's own numbering says which process it
/// is. The launch knows, so the launch states it, in its own numbering; no backend translates
/// between two spaces. Several workers naming one leader is the launcher's arrangement of one sink
/// per host, and nothing here reads anything more into it.
///
/// Built only by [`Deployment::new`], so every value a backend receives has already been checked
/// for the contradictions a backend could otherwise only resolve by guessing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deployment<'a> {
    workers: &'a [Launch],
    leaders: Option<&'a [Launch]>,
}

impl<'a> Deployment<'a> {
    /// `workers` in contract-rank order: the process at position `i` has contract rank `i`.
    /// `leaders`, when present, is parallel to it: position `i` is led by `leaders[i]`. `None` is
    /// a deployment with no leader route.
    ///
    /// The other half of the check — a number outside the job — is the backend's, because only
    /// the backend knows how large its job is.
    pub fn new(workers: &'a [Launch], leaders: Option<&'a [Launch]>) -> Result<Self, Invalid> {
        if workers.is_empty() {
            return Err(Invalid::EmptyDeployment);
        }
        // A position in a list wider than `u32::MAX` cannot be a `Rank`, and every later
        // narrowing of one is sound only because this refuses first.
        if workers.len() > u32::MAX as usize
            || leaders.is_some_and(|leaders| leaders.len() > u32::MAX as usize)
        {
            return Err(Invalid::Unrepresentable);
        }
        if workers
            .iter()
            .enumerate()
            .any(|(at, worker)| workers[..at].contains(worker))
        {
            return Err(Invalid::DuplicateWorker);
        }
        if let Some(leaders) = leaders {
            if leaders.len() != workers.len() {
                return Err(Invalid::UnequalLists);
            }
            if workers.iter().any(|worker| leaders.contains(worker)) {
                return Err(Invalid::WorkerIsLeader);
            }
        }
        Ok(Deployment { workers, leaders })
    }

    pub(crate) fn workers(self) -> &'a [Launch] {
        self.workers
    }

    pub(crate) fn leaders(self) -> Option<&'a [Launch]> {
        self.leaders
    }

    /// Which contract rank `launch` is, if it names a worker. A search, because the declaration
    /// is the authority on the order.
    // Read only by the backends that need it; which ones is not this file's to name.
    #[allow(dead_code)]
    pub(crate) fn contract(self, launch: Launch) -> Option<Rank> {
        self.workers
            .iter()
            .position(|&worker| worker == launch)
            .map(|at| Rank(at as u32))
    }

    /// This worker's leader, by contract rank.
    // Read only by the backends that need it; which ones is not this file's to name.
    #[allow(dead_code)]
    pub(crate) fn leader_of(self, contract: Rank) -> Option<Launch> {
        self.leaders?.get(contract.0 as usize).copied()
    }

    /// The distinct leaders in order of first appearance: one per host. The one rule for host
    /// order, so an application numbering its hosts reads this rather than keeping a copy.
    pub fn hosts(self) -> impl Iterator<Item = Launch> + 'a {
        let leaders = self.leaders.unwrap_or(&[]);
        (0..leaders.len()).filter_map(move |at| {
            let leader = leaders[at];
            (!leaders[..at].contains(&leader)).then_some(leader)
        })
    }

    /// The position of `leader` among the distinct leaders, in order of first appearance. Both
    /// ends of a bridge derive their pairing from this one computation, and an application that
    /// numbers its hosts by leader reads the same one rather than keeping a copy that can diverge.
    pub fn leader_index(self, leader: Launch) -> Option<usize> {
        let leaders = self.leaders?;
        let first = leaders.iter().position(|&l| l == leader)?;
        Some(
            leaders[..first]
                .iter()
                .enumerate()
                .filter(|&(at, earlier)| !leaders[..at].contains(earlier))
                .count(),
        )
    }
}

/// An input the backend refuses, by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// A deployment with no workers.
    EmptyDeployment,
    /// A leader list that is not parallel to the worker list.
    UnequalLists,
    /// A rank named twice as a worker.
    DuplicateWorker,
    /// A rank named both as a worker and as a leader.
    WorkerIsLeader,
    /// A rank this job, cohort or route has no place for.
    RankOutsideJob,
    /// A process that opened a leader route the deployment does not give it.
    WrongLeader,
    /// A leader-route call where the deployment named no leader.
    NoLeader,
    /// A receive from a `concurrent!` arm that has no `recv` setting.
    NotReceiving,
    /// A lane send to a rank the current load gave no lane.
    NoLane,
    /// Lane traffic before `reshape` declared a lane.
    LaneNotConfigured,
    /// A worker list that is not strictly ascending.
    UnorderedWorkers,
    /// An edge list that is not strictly ascending by `(source, destination)`.
    UnorderedEdges,
    /// An edge whose endpoint is not one of the load's workers.
    EdgeOutsideWorkers,
    /// A lane geometry the launched storage cannot hold.
    UnsupportedGeometry,
    /// An `attach` whose mapped header disagrees with the handle in format, length or revision.
    /// POSIX open failures, including ENOENT, carry their errno in BackendFault::Os.
    NoSegment,
    /// A value that does not fit the width the transport carries it in.
    Unrepresentable,
    /// The environment the launch supplied disagrees with itself: header, sizes, alignment, or
    /// leader. Consistency is what is checked; whether its pointers are real allocations is not.
    InconsistentLaunch,
}

/// A failure the backend observed below the contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendFault {
    /// The transport library reported an error.
    Transport,
    /// Storage the backend needs could not be obtained or is too small: an entry that cannot hold
    /// a `MAX_FRAME` frame, or a segment the leader could not create.
    Storage,
    /// A lock guarding backend state was poisoned by a panicking holder.
    Internal,
    /// The POSIX errno of the failing syscall.
    Os(i32),
    /// A contract operation this backend does not implement yet: nv segment publication.
    Unimplemented,
    /// An input refused where there was no call to return it from.
    Invalid(Invalid),
}

/// Whose fault a [`Failure`] is: the backend's, or the application's own `A`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind<A> {
    Backend(BackendFault),
    Application(A),
}

/// Who observed a [`Failure`].
///
/// A leader has no contract rank, and a process refused at entry has not been given one, so a bare
/// `Rank` would have to invent one for them; rank zero invented is worker zero blamed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Participant {
    /// A worker, by contract rank.
    Worker(Rank),
    /// A leader, by its launch rank.
    Leader(Launch),
    /// A process inside `init` or `Leader::open` that has no contract identity yet; `None` means
    /// the transport has not yet assigned a launch rank.
    Entering(Option<Launch>),
}

impl fmt::Display for Participant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Participant::Worker(rank) => write!(f, "worker {}", rank.0),
            Participant::Leader(rank) => write!(f, "leader at launch rank {}", rank.0),
            Participant::Entering(Some(rank)) => write!(f, "launch rank {}", rank.0),
            Participant::Entering(None) => f.write_str("a process with no launch rank"),
        }
    }
}

/// A failure a participant can observe and record.
///
/// A value the participant produces rather than a channel the backend writes to: stderr does not
/// exist on a device, and a failure no participant can observe — a trap, a lost device — is the
/// launcher's to report as abnormal termination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure<A = Infallible> {
    /// The participant that observed it.
    pub participant: Participant,
    /// The operation it was in, as a name a reader can find in the source.
    pub operation: &'static str,
    pub kind: FailureKind<A>,
}

/// Every refusal and failure one attempt can report.
///
/// `Full` is capacity and `Busy` is one contended instant, because waiting helps one and not the
/// other; `TooSmall` is the caller's buffer and `TooLarge` is the frame, because the fixes are
/// opposite. Only an empty receive is `Ok(None)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No capacity at this instant. Nothing was accepted.
    Full,
    /// The backend is mid-operation on this route; the same call may succeed immediately.
    Busy,
    /// The peer is gone.
    Closed,
    /// The frame exceeds what this route can carry.
    TooLarge { limit: usize },
    /// The caller's buffer is smaller than the frame. Nothing was consumed.
    TooSmall { needed: usize },
    Invalid(Invalid),
    /// A failure the participant observed. Fails the run; it is not a safe retry.
    Failed(Failure),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Full => f.write_str("no capacity"),
            Error::Busy => f.write_str("busy"),
            Error::Closed => f.write_str("the peer is gone"),
            Error::TooLarge { limit } => write!(f, "the frame exceeds the {}-byte limit", limit),
            Error::TooSmall { needed } => write!(f, "the buffer needs {} bytes", needed),
            Error::Invalid(why) => write!(f, "invalid input: {why:?}"),
            Error::Failed(failure) => write!(
                f,
                "{} failed in {}: {:?}",
                failure.participant, failure.operation, failure.kind
            ),
        }
    }
}

/// Which route a frame takes.
///
/// The route, not the frame, carries the delivery guarantee: `Message` is reliable and FIFO per
/// `(source, destination, tag)`, while only `Lane` may lose a frame, and only where [`LOSSY`]
/// says so.
///
/// [`LOSSY`]: crate::LOSSY
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Message(Tag),
    Lane,
}

/// One received frame, without its bytes: those were written into the caller's buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    source: Option<Rank>,
    tag: Tag,
    len: usize,
}

impl Frame {
    // Built only by a backend that carries frames.
    #[allow(dead_code)]
    pub(crate) const fn new(source: Option<Rank>, tag: Tag, len: usize) -> Self {
        Frame { source, tag, len }
    }

    /// The sending worker, or `None` for this worker's leader, which has no rank.
    pub const fn source(&self) -> Option<Rank> {
        self.source
    }

    pub const fn tag(&self) -> Tag {
        self.tag
    }

    pub const fn len(&self) -> usize {
        self.len
    }
}

/// One directed lane pair, as declared to `reshape`.
///
/// `affected` is the number of destination elements reachable from `source`. It sizes the pair's
/// depth, and zero cannot size one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    source: Rank,
    destination: Rank,
    affected: NonZeroU32,
}

impl Edge {
    pub fn new(source: Rank, destination: Rank, affected: NonZeroU32) -> Self {
        Edge {
            source,
            destination,
            affected,
        }
    }

    // Read only by the backends that need it; which ones is not this file's to name.
    #[allow(dead_code)]
    pub(crate) fn source(self) -> Rank {
        self.source
    }

    // Read only by the backends that need it; which ones is not this file's to name.
    #[allow(dead_code)]
    pub(crate) fn destination(self) -> Rank {
        self.destination
    }

    // Read only by the backends that need it; which ones is not this file's to name.
    #[allow(dead_code)]
    pub(crate) fn affected(self) -> NonZeroU32 {
        self.affected
    }
}

/// A span of nanoseconds. Integer because a device counter is an integer count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    nanos: u64,
}

impl Span {
    pub const fn from_nanos(nanos: u64) -> Self {
        Span { nanos }
    }

    pub const fn from_millis(millis: u64) -> Self {
        Span {
            nanos: millis.saturating_mul(1_000_000),
        }
    }

    pub const fn nanos(self) -> u64 {
        self.nanos
    }
}

/// The comparison domain of a clock, including its incarnation. Not a rank, because two runs on
/// one rank are different clocks, and not a process id alone, because that is reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockId {
    incarnation: u32,
}

impl ClockId {
    pub(crate) const fn new(incarnation: u32) -> Self {
        ClockId { incarnation }
    }
}

/// Two readings of different clocks were subtracted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockMismatch;

/// A clock reading: a span from an origin, tagged with the identity it is comparable within.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    clock: ClockId,
    elapsed: Span,
}

impl Reading {
    pub(crate) const fn new(clock: ClockId, elapsed: Span) -> Self {
        Reading { clock, elapsed }
    }

    pub const fn clock(self) -> ClockId {
        self.clock
    }

    pub const fn elapsed(self) -> Span {
        self.elapsed
    }

    /// The span between two readings of one clock, saturating at zero. A silent zero across
    /// clocks would be indistinguishable from a span that really was zero, so that is refused.
    pub fn since(self, earlier: Reading) -> Result<Span, ClockMismatch> {
        if self.clock != earlier.clock {
            return Err(ClockMismatch);
        }
        Ok(Span::from_nanos(
            self.elapsed.nanos.saturating_sub(earlier.elapsed.nanos),
        ))
    }
}

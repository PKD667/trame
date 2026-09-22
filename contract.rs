// The value types every backend shares.
//
// These are here rather than in each backend because they carry no backend choice: `Frame` is a
// source, a tag and a length wherever it came from, and `Error` is the set of refusals the surface
// promises to distinguish. `Context`, `Environment` and `Shared` are *not* here, because those are
// where a backend's storage, identity and lifetime actually differ.
//
// `Rank`, `Tag` and `FrameBytes` are newtypes with private fields so that a rank cannot be passed
// where a byte count was meant, and a backend converts each at its own boundary with a check: a
// value that does not fit is an `Invalid` rather than a truncation, because a truncated rank is a
// frame delivered to the wrong participant.
//
// `Span` is an integer count of nanoseconds and deliberately not `std::time::Duration`: a device
// has no such type, and a duration that carries its own clock is a duration that can be subtracted
// from the wrong one. Only readings sharing a `ClockId` may be compared.

use core::convert::Infallible;
use core::fmt;
use core::num::NonZeroU32;

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

/// A frame's length in bytes. Transport counts are `u32` on every backend, so a `usize` length is
/// converted once, checked, where it enters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameBytes(u32);

impl FrameBytes {
    pub(crate) const fn new(bytes: u32) -> Self {
        FrameBytes(bytes)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<usize> for FrameBytes {
    type Error = Invalid;

    fn try_from(bytes: usize) -> Result<Self, Invalid> {
        u32::try_from(bytes)
            .map(FrameBytes)
            .map_err(|_| Invalid::Unrepresentable)
    }
}

/// A participant's bytes in a published segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteRange {
    pub offset: usize,
    pub length: usize,
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
    workers: &'a [Rank],
    leaders: Option<&'a [Rank]>,
}

impl<'a> Deployment<'a> {
    /// `workers` in contract-rank order: the process at position `i` has contract rank `i`.
    /// `leaders`, when present, is parallel to it: position `i` is led by `leaders[i]`. `None` is
    /// a deployment with no leader route.
    ///
    /// The other half of the check — a number outside the job — is the backend's, because only
    /// the backend knows how large its job is.
    pub fn new(workers: &'a [Rank], leaders: Option<&'a [Rank]>) -> Result<Self, Invalid> {
        if workers.is_empty() {
            return Err(Invalid::EmptyDeployment);
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

    pub(crate) fn workers(self) -> &'a [Rank] {
        self.workers
    }

    pub(crate) fn leaders(self) -> Option<&'a [Rank]> {
        self.leaders
    }

    /// Which contract rank `rank` is, if it names a worker. A search, because the declaration is
    /// the authority on the order.
    #[cfg_attr(not(any(feature = "mpi", feature = "nv")), allow(dead_code))]
    pub(crate) fn contract(self, rank: Rank) -> Option<Rank> {
        self.workers
            .iter()
            .position(|&worker| worker == rank)
            .map(|at| Rank(at as u32))
    }

    /// This worker's leader, by contract rank.
    #[cfg_attr(not(any(feature = "mpi", feature = "nv")), allow(dead_code))]
    pub(crate) fn leader_of(self, contract: Rank) -> Option<Rank> {
        self.leaders?.get(contract.0 as usize).copied()
    }

    /// The position of `leader` among the distinct leaders, in order of first appearance. Both
    /// ends of a bridge derive their pairing from this one computation.
    #[cfg_attr(not(feature = "mpi"), allow(dead_code))]
    pub(crate) fn leader_index(self, leader: Rank) -> Option<usize> {
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
    /// A `share` whose bytes are not this participant's slice.
    BadShareLength,
    /// A `share` with no launch segment to publish into, or one too small.
    MissingSegment,
    /// A value that does not fit the width the transport carries it in.
    Unrepresentable,
}

/// A failure the backend observed below the contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendFault {
    /// The transport library reported an error.
    Transport,
    /// The storage the entry supplied cannot hold a `MAX_FRAME` frame.
    Storage,
    /// A lock guarding backend state was poisoned by a panicking holder.
    Internal,
    /// An input refused where there was no call to return it from.
    Invalid(Invalid),
}

/// Whose fault a [`Failure`] is: the backend's, or the application's own `A`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind<A> {
    Backend(BackendFault),
    Application(A),
}

/// A failure a participant can observe and record.
///
/// A value the participant produces rather than a channel the backend writes to: stderr does not
/// exist on a device, and a failure no participant can observe — a trap, a lost device — is the
/// launcher's to report as abnormal termination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure<A = Infallible> {
    /// The participant that observed it.
    pub participant: Rank,
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
    TooLarge { limit: FrameBytes },
    /// The caller's buffer is smaller than the frame. Nothing was consumed.
    TooSmall { needed: FrameBytes },
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
            Error::TooLarge { limit } => write!(f, "the frame exceeds the {}-byte limit", limit.0),
            Error::TooSmall { needed } => write!(f, "the buffer needs {} bytes", needed.0),
            Error::Invalid(why) => write!(f, "invalid input: {why:?}"),
            Error::Failed(failure) => write!(
                f,
                "participant {} failed in {}: {:?}",
                failure.participant.0, failure.operation, failure.kind
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
    source: Rank,
    tag: Tag,
    len: FrameBytes,
}

impl Frame {
    #[cfg_attr(not(any(feature = "mpi", feature = "nv")), allow(dead_code))]
    pub(crate) const fn new(source: Rank, tag: Tag, len: FrameBytes) -> Self {
        Frame { source, tag, len }
    }

    pub const fn source(&self) -> Rank {
        self.source
    }

    pub const fn tag(&self) -> Tag {
        self.tag
    }

    pub const fn len(&self) -> FrameBytes {
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

    #[cfg_attr(not(any(feature = "mpi", feature = "nv")), allow(dead_code))]
    pub(crate) fn source(self) -> Rank {
        self.source
    }

    #[cfg_attr(not(any(feature = "mpi", feature = "nv")), allow(dead_code))]
    pub(crate) fn destination(self) -> Rank {
        self.destination
    }

    #[cfg_attr(not(any(feature = "ring", feature = "nv")), allow(dead_code))]
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

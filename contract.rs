// The value types every backend shares, and the capability mechanism the declarations require.
//
// These are here rather than in each backend because they carry no backend choice: `Wait` is one
// attempt or a declared wait on every transport, `Frame` is a source, a tag and a length
// wherever it came from, and `Error` is the set of refusals the surface promises to distinguish.
// `Context`, `Environment` and `Shared` are *not* here, because those are where a backend's
// storage, identity and lifetime actually differ.
//
// `Rank` and `Tag` are `u32` on every backend, including the MPI ones, whose native rank is a
// signed `int`. A backend converts at its own boundary and checks the conversion: a rank that
// does not fit is an `Invalid` rather than a truncation, because a truncated rank is a frame
// delivered to the wrong participant and the spec's whole point is that a fault should be
// unambiguous about where it lives.
//
// `Span` is an integer count of nanoseconds and deliberately not `std::time::Duration`: a device
// has no such type, and a duration that carries its own clock is a duration that can be subtracted
// from the wrong one. The clock rules require the two to be separate so that only readings sharing
// a `ClockId` may be compared.

use core::fmt;

/// A participant's dense index in the cohort, `[0, size)`.
pub type Rank = u32;

/// A frame's tag. The backend declares its limit; see [`Capabilities::tag_limit`].
pub type Tag = u32;

/// What the launch says about who is who.
///
/// A leader is not a participant, so nothing in a participant's own numbering says which process it
/// is, and a backend that derived a role from a rank would be inventing a fact rather than reading
/// one. The launch knows, so the launch states, and this is where it states it.
///
/// Both fields are in the launch's own numbering — on a device backend that is the launch's warp or
/// block index and not an MPI rank — so no backend translates between two spaces, and a backend
/// that had to would be one that could get it wrong.
///
/// Two parallel lists rather than one leader, because a deployment may have more than one: the
/// decision that there is one sink per host is the *launcher's*, and it shows up here only as
/// several workers naming the same leader. A contract that knew what a host was would be encoding a
/// topology it has no other use for.
///
/// An empty `workers` means *every participant this backend has*, not none, and it implies an empty
/// `leaders`: every participant works and there is no leader route. The list is a narrowing and
/// empty is the widening, which is what keeps this type single-valued where the degenerate cases
/// meet — a leaderless job and a backend with no peers both state nothing, and each has exactly one
/// reading of that. The two lists are the same length or the deployment is refused, so there is no
/// shape between the two in which anything would have to be guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deployment<'a> {
    /// The workers, in contract-rank order: the launch's process at position `i` has contract rank
    /// `i`. The order is part of the declaration rather than an accident of gathering, because it
    /// is what a contract rank *is*.
    pub workers: &'a [Rank],
    /// One entry per worker, parallel to `workers`: the launch's process at position `i` is led by
    /// `leaders[i]`. Which workers share a leader is the launcher's arrangement, and nothing here
    /// reads anything into it.
    pub leaders: &'a [Rank],
}

impl<'a> Deployment<'a> {
    /// The deployment that states nothing: every participant the backend has is a worker, and
    /// there is no leader.
    pub const SOLIDARY: Deployment<'static> = Deployment {
        workers: &[],
        leaders: &[],
    };

    /// Whether `rank` names a worker, and which contract rank it is.
    ///
    /// A search rather than arithmetic, because the declaration is the authority on the order and
    /// a second rule deriving it would be a second answer that can disagree with the first.
    pub fn contract(self, rank: Rank) -> Option<Rank> {
        self.workers
            .iter()
            .position(|&worker| worker == rank)
            .map(|at| at as Rank)
    }

    /// This worker's leader, by contract rank.
    pub fn leader_of(self, contract: Rank) -> Option<Rank> {
        self.leaders.get(contract as usize).copied()
    }

    /// The position of `leader` among the distinct leaders, counted in order of first appearance.
    ///
    /// Both ends of a bridge derive their colour from this, so the pairing is one computation
    /// rather than two that can disagree about which group is which — which is the failure that
    /// would show up as two leaders bridged to each other's workers.
    pub fn leader_index(self, leader: Rank) -> Option<usize> {
        let first = self.leaders.iter().position(|&l| l == leader)?;
        Some(
            self.leaders[..first]
                .iter()
                .enumerate()
                .filter(|&(at, &earlier)| !self.leaders[..at].contains(&earlier))
                .count(),
        )
    }

    /// Why this deployment cannot be acted on, if it cannot.
    ///
    /// Two facts stated as one have to agree, and a backend that resolves the disagreement instead
    /// of reporting it delivers frames according to a rule nobody wrote down. The cases are a list
    /// of leaders that does not line up with the list of workers, and a rank claimed as both. The
    /// other half of the check — a number outside the job — is the backend's, because only the
    /// backend knows how large its job is.
    pub fn refuse(self) -> Option<&'static str> {
        if self.workers.len() != self.leaders.len() {
            return Some("the deployment's worker list and leader list are of different lengths");
        }
        if self
            .workers
            .iter()
            .any(|worker| self.leaders.contains(worker))
        {
            return Some("the deployment names one rank as both a leader and a worker");
        }
        None
    }
}

/// One attempt, or the backend's declared waiting policy.
///
/// `Poll` is a bounded single attempt and never waits for a peer. `Wait` is whatever the backend
/// declared: blocking, or at most a stated number of attempts. The two are separate variants
/// rather than a budget because "do not wait" and "wait on a peer that may not be running" are
/// different requests, and a caller that means the first must not be given the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    Poll,
    Wait,
}

/// Which route a frame takes.
///
/// The route, not the frame, carries the delivery guarantee: `Message` is reliable and FIFO per
/// `(source, destination, tag)`, while `Lane` is whatever the geometry's backend declared. A name
/// for the route is what keeps a lossy lane from being read as permission to lose a control frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Message(Tag),
    Lane,
}

/// One received frame, without its bytes.
///
/// The bytes were written into the caller's buffer, so a frame that outlives the call would be a
/// frame with no bytes. It is plain data for exactly that reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub source: Rank,
    pub tag: Tag,
    pub len: u32,
}

/// One directed lane pair, as declared to `reshape`.
///
/// `affected` is the number of destination elements reachable from `source`. It sizes the pair's
/// depth in a backend that can size per pair; a backend with one launch-wide geometry validates
/// the declaration against it instead, which is why the field is required even where it is not
/// used to allocate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub source: Rank,
    pub destination: Rank,
    pub affected: u32,
}

/// A span of nanoseconds.
///
/// Integer nanoseconds because a device counter is an integer count and a conversion to a float
/// duration would introduce a rounding the spec did not ask for. The backend states its
/// resolution and how it converts from native ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    pub nanos: u64,
}

impl Span {
    #[inline]
    pub const fn from_nanos(nanos: u64) -> Self {
        Span { nanos }
    }

    /// The span a duration constant names. These exist so that a `Duration`-shaped constant is a
    /// rename rather than a rewrite: the arithmetic is the same, and only the clock attached to
    /// `std::time::Duration` goes away.
    #[inline]
    pub const fn from_micros(micros: u64) -> Self {
        Span {
            nanos: micros.saturating_mul(1_000),
        }
    }

    #[inline]
    pub const fn from_millis(millis: u64) -> Self {
        Span {
            nanos: millis.saturating_mul(1_000_000),
        }
    }

    #[inline]
    pub const fn from_secs(secs: u64) -> Self {
        Span {
            nanos: secs.saturating_mul(1_000_000_000),
        }
    }

    /// The saturating difference, so that a span never wraps into a large positive one.
    #[inline]
    pub const fn since(self, earlier: Span) -> Self {
        Span {
            nanos: self.nanos.saturating_sub(earlier.nanos),
        }
    }
}

/// The comparison domain of a clock, including its incarnation.
///
/// Two readings may be subtracted only if their identities are equal. The identity is not a
/// participant rank and not a process id by definition: it is whatever value the backend gives
/// two readings that share one origin, and the backend states how it is formed. A rank would be
/// wrong because two runs on one rank are different clocks; a process id would be wrong because
/// it is reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockId {
    pub incarnation: u32,
}

/// A clock reading: a span from an origin, tagged with the identity it is comparable within.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub clock: ClockId,
    pub elapsed: Span,
}

impl Reading {
    /// The span between two readings of one clock. Saturates to zero rather than wrapping.
    ///
    /// # Panics
    ///
    /// If the two readings do not share a clock identity. Subtracting across clocks is the error
    /// the identity exists to make impossible, and a silent zero would be indistinguishable from
    /// a span that really was zero.
    #[inline]
    pub fn since(self, earlier: Reading) -> Span {
        assert!(
            self.clock == earlier.clock,
            "readings from different clock identities may not be subtracted"
        );
        self.elapsed.since(earlier.elapsed)
    }
}

/// A failure a participant can observe and record.
///
/// The record is the device form of a diagnosis: stderr does not exist on a device, and a trap
/// takes the launch with it before anything can be written. A failure that a participant cannot
/// observe at all — a trap, a lost device — is reported by the launcher as abnormal termination
/// and is not a record, which is why this is a value the participant produces rather than a
/// channel the backend writes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The participant that observed it.
    pub participant: Rank,
    /// The operation it was in, as a name a reader can find in the source.
    pub operation: &'static str,
    /// A backend-defined diagnosis code.
    pub code: u32,
}

/// Every refusal and failure the communication surface can report.
///
/// The distinctions are the ones a retry loop needs, and each pair is separate for a reason:
/// `Full` is capacity and `Busy` is one contended instant, because waiting helps one and not the
/// other; `TooSmall` is the caller's buffer and `TooLarge` is the frame, because the fixes are
/// opposite; `Exhausted` is a bounded wait that ran out and `Closed` is a peer that is gone,
/// because retrying helps neither but only one is the caller's to fix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No capacity at this instant.
    Full,
    /// The backend is mid-operation on this route; the same call may succeed immediately.
    Busy,
    /// The peer is gone.
    Closed,
    /// The frame exceeds what this backend can carry.
    TooLarge { limit: u32 },
    /// The caller's buffer is smaller than the frame. Nothing was consumed.
    TooSmall { needed: u32 },
    /// A bounded wait used its whole budget.
    Exhausted { attempts: u32 },
    /// An input the backend cannot interpret. `code` names which.
    Invalid { code: u32 },
    /// A failure the participant observed. Fails the run; it is not a safe retry.
    Failed(Failure),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Full => f.write_str("no capacity"),
            Error::Busy => f.write_str("busy"),
            Error::Closed => f.write_str("the peer is gone"),
            Error::TooLarge { limit } => write!(f, "the frame exceeds the {limit}-byte limit"),
            Error::TooSmall { needed } => write!(f, "the buffer needs {needed} bytes"),
            Error::Exhausted { attempts } => write!(f, "the wait used {attempts} attempts"),
            Error::Invalid { code } => write!(f, "invalid input, code {code}"),
            Error::Failed(failure) => write!(
                f,
                "participant {} failed in {}: code {}",
                failure.participant, failure.operation, failure.code
            ),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// What a backend declares

// The closed value sets are plain `u8` and `bool` with named constants rather than an enum per
// set. The sets are closed because `require!` has one arm per name and refuses anything else, and
// that refusal is what "closed" has to mean to a caller — an enum would add nine types to the
// tree to say the same thing, and the complexity gate is right that nine types is a cost.

/// A lane delivers every accepted frame. Overwrites nothing.
pub const LANE_RELIABLE: u8 = 0;
/// A lane may overwrite an unread frame. The `Message` route is unaffected.
pub const LANE_LOSSY: u8 = 1;
/// The backend has no lane route at all.
pub const LANE_UNAVAILABLE: u8 = 2;

/// `release` discharges the caller's own obligation and says nothing about peers.
pub const RELEASE_LOCAL: u8 = 0;
/// `release` is cohort-wide quiescence: every member has stopped touching the storage.
pub const RELEASE_COHORT: u8 = 1;
/// The backend has no lane storage to release.
pub const RELEASE_UNAVAILABLE: u8 = 2;

/// `share` is a collective whose members call it together.
pub const PUBLICATION_COLLECTIVE: u8 = 0;
/// The segment is an allocation installed before participants entered, so publication is a write.
pub const PUBLICATION_PREPUBLISHED: u8 = 1;
/// The backend has no sharing.
pub const PUBLICATION_UNAVAILABLE: u8 = 2;

/// The visibility scopes of the supported atomics.
///
/// `domain` means a declared sharing domain, which is the only scope wider than a participant
/// that the contract can name without naming a machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scopes {
    pub participant: bool,
    pub domain: bool,
    pub system: bool,
}

/// Everything a backend has to publish.
///
/// A declaration is not documentation. Each `true` here has an implementation behind it, each
/// `false` has a refusal, and `require!` turns a program's needs into a build failure rather than
/// a run-time disappointment. A claimed capability without that refusal is insufficient, and so is
/// a refusal that only a run reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declarations {
    pub atomic_scopes: Scopes,
    /// One of [`LANE_RELIABLE`], [`LANE_LOSSY`], [`LANE_UNAVAILABLE`].
    pub lane_reliability: u8,
    /// Attempts before a wait reports `Exhausted`, per route. `0` is a blocking policy; `n > 0` is
    /// at most `n` attempts. The wait policy requires the bound to be stated, and zero attempts is
    /// not a bound, so zero can only mean blocking.
    ///
    /// Per route because one backend's routes need not share a policy: `rma`'s lane route has to
    /// spin on an acknowledgement while its message route copies and returns.
    pub waiting_message: u32,
    pub waiting_lane: u32,
    /// Whether a refused attempt on this route can report `Full`. A route whose admission
    /// capacity cannot be observed *at all* is `false`, and a caller that depends on flow control
    /// requires `backpressure` rather than reading the absence of `Full` as room.
    ///
    /// "At all" is the test, not "on every path": a route that reports `Full` from a polling
    /// attempt while its blocking attempt waits for room is still reported, because the capacity
    /// is observable.
    pub pressure_message: bool,
    pub pressure_lane: bool,
    /// One of [`RELEASE_LOCAL`], [`RELEASE_COHORT`], [`RELEASE_UNAVAILABLE`].
    pub release: u8,
    /// One of [`PUBLICATION_COLLECTIVE`], [`PUBLICATION_PREPUBLISHED`],
    /// [`PUBLICATION_UNAVAILABLE`].
    pub publication: u8,
    /// `false` is `equal_only`: every caller shares the default priority and the waiter table's
    /// priority ordering has nothing to order. `true` is `ordered`.
    pub priority: bool,
    /// `false` is `spawned`, `true` is `resident`.
    pub resident: bool,
    /// The longest frame one send may carry. A launch-time arena declares `u32::MAX` and checks
    /// the real limit in `reshape`, because a compile-time number nobody could honour is worse
    /// than an honest bound the geometry is validated against.
    pub max_frame: u32,
    pub tag_limit: Tag,
    /// The lowering grammar a program is compiled against, by identifier.
    ///
    /// An identifier names a *specified subset* and never implies arbitrary Rust.
    pub lowering: &'static str,
}

impl Declarations {
    /// Whether a requirement that holds per route is answered on every route this backend serves.
    ///
    /// `waiting` and `pressure` are declared per route because a backend's routes need not share a
    /// policy. A route it does not serve cannot be waited on, so a requirement about waiting is
    /// vacuous there, and `lane_reliability == LANE_UNAVAILABLE` is exactly the declaration that
    /// says the lane route does not exist. Reading an absent route's numbers as claims would be a
    /// report the backend did not make.
    pub const fn per_route(&self, message: bool, lane: bool) -> bool {
        message && (self.lane_reliability == LANE_UNAVAILABLE || lane)
    }
}

/// The check `require!` expands to: a length in a type, so it is evaluated while the crate is
/// type-checked rather than when a body is code generated.
#[doc(hidden)]
#[macro_export]
macro_rules! __require {
    ($cond:expr, $name:literal) => {
        #[doc(hidden)]
        #[allow(dead_code, non_upper_case_globals)]
        const _: [(); 0] = [(); {
            assert!(
                $cond,
                concat!(
                    "this program requires `",
                    $name,
                    "`, which the selected backend does not declare"
                )
            );
            0
        }];
    };
}

/// Refuse the build unless the selected backend declares what the name asks for.
///
/// The accepted names are operations, families, and the property requirements the contract names.
/// The set is closed, and that is the point: a requirement spelled wrongly is a macro error rather
/// than a requirement silently satisfied.
///
/// One requirement per invocation. A list would need a recursive arm, and a recursive arm is more
/// macro than a caller's convenience is worth — `require!(send); require!(recv);` says the same
/// thing and expands to two independent checks.
///
/// ```ignore
/// trame::require!(send);
/// trame::require!(reliable_lanes);
/// trame::require!(atomic_scope(domain));
/// ```
#[macro_export]
macro_rules! require {
    (reliable_lanes) => {
        $crate::__require!(
            $crate::DECLARATIONS.lane_reliability == $crate::LANE_RELIABLE,
            "reliable_lanes"
        );
    };
    (bounded_wait) => {
        $crate::__require!(
            $crate::DECLARATIONS.per_route(
                $crate::DECLARATIONS.waiting_message > 0,
                $crate::DECLARATIONS.waiting_lane > 0,
            ),
            "bounded_wait"
        );
    };
    (backpressure) => {
        $crate::__require!(
            $crate::DECLARATIONS.per_route(
                $crate::DECLARATIONS.pressure_message,
                $crate::DECLARATIONS.pressure_lane,
            ),
            "backpressure"
        );
    };
    (global_release) => {
        $crate::__require!(
            $crate::DECLARATIONS.release == $crate::RELEASE_COHORT,
            "global_release"
        );
    };
    (collective_share) => {
        $crate::__require!(
            $crate::DECLARATIONS.publication == $crate::PUBLICATION_COLLECTIVE,
            "collective_share"
        );
    };
    (priority) => {
        $crate::__require!($crate::DECLARATIONS.priority, "priority");
    };
    (atomic_scope(participant)) => {
        $crate::__require!(
            $crate::DECLARATIONS.atomic_scopes.participant,
            "atomic_scope(participant)"
        );
    };
    (atomic_scope(domain)) => {
        $crate::__require!(
            $crate::DECLARATIONS.atomic_scopes.domain,
            "atomic_scope(domain)"
        );
    };
    (atomic_scope(system)) => {
        $crate::__require!(
            $crate::DECLARATIONS.atomic_scopes.system,
            "atomic_scope(system)"
        );
    };
}

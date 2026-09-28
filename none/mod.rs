// The backend that does nothing, so a build can link the surface with no transport at all.
//
// One worker, Launch(0), and optionally a leader, Launch(1). Nothing moves: every send is refused
// as `Closed`, because nothing is on the other end, and nothing ever arrives. The entry, the
// clock, the shared-state families and a segment its leader publishes still work, because a
// process alone answers them.

use std::cell::UnsafeCell;
use std::marker::PhantomData;

use crate::contract::{
    Backend, BackendFault, Channel, Deployment, Edge, Error, Failure, FailureKind, Frame, Handle,
    Invalid, Launch, Participant, Rank, Tag,
};
use crate::invoke::{Owner, Receive};

pub use crate::cpu::clock;
pub use crate::cpu::run;
pub use crate::cpu::sync;

pub const ID: Backend = Backend::None;

/// The contract's floor: a 64 KiB log block behind an eight-byte batch header.
pub const MAX_FRAME: usize = 65_544;

const ME: Rank = Rank::from_index(0);

/// The one process's worker launch rank.
const LAUNCH: Launch = Launch::new(0);

/// The optional leader's launch rank. Never a worker: a launch is one or the other.
const LEADER: Launch = Launch::new(1);

/// What the entry is given: nothing, since there is no transport to describe.
#[derive(Clone, Default)]
pub struct Environment;

/// This participant's state: whether the deployment named it a leader, and itself as the whole of
/// `hosts` and `cohort`.
pub struct Context {
    alone: [Rank; 1],
    led: bool,
    // Auto traits match the nv backend's, so the public surface is the same on every backend.
    _unshared: PhantomData<*const ()>,
}

// SAFETY: every field is `Send`; the marker exists only to withdraw `Sync`, as nv's pointers do.
unsafe impl Send for Context {}

/// Whether `deployment` is this launch of one, and whether it names the leader.
fn launch_of_one(deployment: Deployment<'_>, who: Launch, operation: &'static str) -> Result<bool, Failure> {
    let refuse = |why| Failure {
        participant: Participant::Entering(Some(who)),
        operation,
        kind: FailureKind::Backend(BackendFault::Invalid(why)),
    };
    let led = match deployment.leaders() {
        None => false,
        Some(leaders) if leaders == [LEADER] => true,
        Some(_) => return Err(refuse(Invalid::RankOutsideJob)),
    };
    if deployment.workers() != [LAUNCH] {
        return Err(refuse(Invalid::RankOutsideJob));
    }
    Ok(led)
}

/// Enter as the worker of a deployment that names that one worker and at most its leader. Any
/// other launch set is refused rather than narrowed.
pub fn init(_env: Environment, deployment: Deployment<'_>) -> Result<Context, Failure> {
    let led = launch_of_one(deployment, LAUNCH, "init")?;
    Ok(Context { alone: [ME], led, _unshared: PhantomData })
}

pub fn rank(_cx: &Context) -> Rank {
    ME
}

pub fn size(_cx: &Context) -> u32 {
    1
}

pub fn hosts(cx: &Context) -> &[Rank] {
    &cx.alone
}

/// One worker has no one to meet, so the phase boundary is already crossed.
pub fn barrier(_cx: &mut Context) {}

/// There is no launch-wide record to discharge, so the outcome is the whole of it.
pub fn done<A>(_cx: &mut Context, outcome: Result<(), Failure<A>>) -> Result<(), Failure<A>> {
    outcome
}

/// Nothing is on the other end: another rank is outside the job, and this one has no route.
fn nowhere(to: Rank) -> Result<(), Error> {
    if to != ME {
        return Err(Error::Invalid(Invalid::RankOutsideJob));
    }
    Err(Error::Closed)
}

/// A leader route that the deployment named, and that goes nowhere.
fn nowhere_up(led: bool) -> Result<(), Error> {
    if !led {
        return Err(Error::Invalid(Invalid::NoLeader));
    }
    Err(Error::Closed)
}

pub fn send(_cx: &mut Context, to: Rank, _channel: Channel, _data: &[u8]) -> Result<(), Error> {
    nowhere(to)
}

pub fn recv(_cx: &mut Context, _out: &mut [u8]) -> Result<Option<Frame>, Error> {
    Ok(None)
}

pub fn flush(_cx: &mut Context) -> Result<(), Error> {
    Ok(())
}

/// A declaration is still checked by its names. It holds no lanes, since nothing is sent on them.
pub fn reshape(
    cx: &mut Context,
    workers: &[Rank],
    edges: &[Edge],
    bytes: usize,
    _tag: Tag,
) -> Result<(), Error> {
    crate::cpu::lanes::validate(workers, edges, bytes, size(cx), MAX_FRAME)
}

pub fn release(_cx: &mut Context) -> Result<(), Error> {
    Ok(())
}

/// The leader's copy, attached by address: the worker is in the leader's process.
pub struct Shared {
    base: *const u8,
    len: usize,
}

/// Attach the leader's segment by address.
///
/// # Safety
/// `handle` came from `leader::handle` of a segment not retired before the returned `Shared` is
/// detached or dropped.
pub unsafe fn attach(_cx: &mut Context, handle: Handle) -> Result<Shared, Error> {
    let len =
        usize::try_from(handle.length()).map_err(|_| Error::Invalid(Invalid::Unrepresentable))?;
    Ok(Shared {
        base: handle.token() as usize as *const u8,
        len,
    })
}

/// The segment, read-only.
pub fn bytes(segment: &Shared) -> &[u8] {
    // SAFETY: attach's promise keeps the leader's Vec alive and unwritten. The Vec owns its heap
    // buffer, which does not move when the Vec value moves, and the owner never mutates or
    // reallocates it after publication.
    unsafe { core::slice::from_raw_parts(segment.base, segment.len) }
}

/// Retire this worker's mapping.
pub fn detach(_cx: &mut Context, _segment: Shared) -> Result<(), (Shared, Error)> {
    Ok(())
}

/// One `concurrent!` arm's end of routes that go nowhere.
pub struct Io<'a> {
    led: bool,
    receives: bool,
    // Auto traits match the nv backend's, so the public surface is the same on every backend.
    _unshared: PhantomData<&'a *const ()>,
}

// SAFETY: every field is `Send`; the marker exists only to withdraw `Sync`, as nv's pointers do.
unsafe impl Send for Io<'_> {}

/// `concurrent!` given a context: the arms still run, each on its own host thread.
#[doc(hidden)]
pub fn concurrent_io<'env, B, E: Send, const N: usize>(
    cx: &'env mut Context,
    receive: &'env [Receive<'env>; N],
    body: B,
) -> Result<(), E>
where
    B: for<'scope> FnOnce(&mut crate::cpu::run::IoArms<'scope, 'env, Io<'env>, E, N>),
{
    let led = cx.led;
    let ios = core::array::from_fn(|at| Io {
        led,
        receives: Owner::new(receive, at).receives(),
        _unshared: PhantomData,
    });
    crate::cpu::run::spawn(ios, body)
}

impl Io<'_> {
    pub fn send(&mut self, to: Rank, _channel: Channel, _data: &[u8]) -> Result<(), Error> {
        nowhere(to)
    }

    pub fn lead(&mut self, _tag: Tag, _data: &[u8]) -> Result<(), Error> {
        nowhere_up(self.led)
    }

    pub fn recv(&mut self, _out: &mut [u8]) -> Result<Option<Frame>, Error> {
        if !self.receives {
            return Err(Error::Invalid(Invalid::NotReceiving));
        }
        Ok(None)
    }

    pub fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

/// The leader route of a launch of one: it opens, and it goes nowhere.
pub mod leader {
    use super::{Context, LEADER, UnsafeCell, launch_of_one, nowhere, nowhere_up};
    use std::marker::PhantomData;
    use std::num::NonZeroU64;
    use crate::contract::{
        BackendFault, Deployment, Error, Failure, FailureKind, Frame, Handle, Invalid, Participant,
        Rank, Tag,
    };

    // No mutable contents: !Freeze, but shared unwind safety remains intact.
    struct Unfrozen(UnsafeCell<()>);
    impl std::panic::RefUnwindSafe for Unfrozen {}

    /// The leader's end, holding no rank.
    pub struct Leader(
        // Auto traits match the nv backend's, so the public surface is the same on every backend.
        PhantomData<*const ()>,
        // The other leader routes are !Freeze; this zero-sized cell has no mutable contents.
        Unfrozen,
    );

    impl Leader {
        /// Open the route of a deployment naming the worker Launch(0) and this leader Launch(1).
        pub fn open(_env: super::Environment, deployment: Deployment<'_>) -> Result<Leader, Failure> {
            if !launch_of_one(deployment, LEADER, "leader::open")? {
                return Err(Failure {
                    participant: Participant::Entering(Some(LEADER)),
                    operation: "leader::open",
                    kind: FailureKind::Backend(BackendFault::Invalid(Invalid::NoLeader)),
                });
            }
            Ok(Leader(PhantomData, Unfrozen(UnsafeCell::new(()))))
        }

        pub fn send(&self, to: Rank, _tag: Tag, _data: &[u8]) -> Result<(), Error> {
            nowhere(to)
        }

        pub fn recv(&self, _out: &mut [u8]) -> Result<Option<Frame>, Error> {
            Ok(None)
        }
    }

    pub fn send(cx: &mut Context, _tag: Tag, _data: &[u8]) -> Result<(), Error> {
        nowhere_up(cx.led)
    }

    pub fn recv(cx: &mut Context, _out: &mut [u8]) -> Result<Option<Frame>, Error> {
        if !cx.led {
            return Err(Error::Invalid(Invalid::NoLeader));
        }
        Ok(None)
    }

    /// The leader's published segment: it owns the bytes its workers attach by address.
    pub struct Published(
        Vec<u8>,
        Handle,
        // Auto traits match the nv backend's, so the public surface is the same on every backend.
        PhantomData<*const ()>,
    );

    /// Publish `bytes` as `revision`: keep a copy and name it by its address in this process.
    pub fn publish(_leader: &Leader, revision: NonZeroU64, bytes: &[u8]) -> Result<Published, Error> {
        let length =
            u64::try_from(bytes.len()).map_err(|_| Error::Invalid(Invalid::Unrepresentable))?;
        let copy: Vec<u8> = bytes.to_vec();
        let address = u64::try_from(copy.as_ptr() as usize)
            .map_err(|_| Error::Invalid(Invalid::Unrepresentable))?;
        Ok(Published(
            copy,
            Handle::new(revision, length, address),
            PhantomData,
        ))
    }

    /// The handle that names `segment` to its workers.
    pub fn handle(segment: &Published) -> Handle {
        segment.1
    }

    /// Retire `segment`.
    pub fn retire(_leader: &Leader, _segment: Published) -> Result<(), (Published, Error)> {
        Ok(())
    }
}

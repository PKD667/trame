// The backend that does nothing, so a build can link the surface with no transport at all.
//
// One worker, Launch(0), and its leader, Launch(1). Nothing moves: every send is refused
// as `Closed`, because nothing is on the other end, and nothing ever arrives. The entry, the
// clock, the shared-state families and a segment its leader publishes still work, because a
// process alone answers them.

pub mod optim;

use std::cell::UnsafeCell;
use std::marker::PhantomData;

use crate::contract::{
    Addr, Backend, BackendFault, Channel, Deployment, Edge, Error, Failure, FailureKind, Frame,
    Handle, Invalid, Launch, Participant, Tag,
};
use crate::invoke::{Owner, Receive};

pub use crate::host::clock;
pub use crate::host::run;
pub use crate::host::sync;

pub const ID: Backend = Backend::None;

/// The contract's floor: a 64 KiB log block behind an eight-byte batch header.
pub const MAX_FRAME: usize = 65_544;

const ME: u32 = 0;

/// The one process's worker launch rank.
const LAUNCH: Launch = Launch::new(0);

/// The leader's launch rank. Never a worker: a launch is one or the other.
const LEADER: Launch = Launch::new(1);

/// What the entry is given: nothing, since there is no transport to describe.
#[derive(Clone, Default)]
pub struct Environment;

/// This participant's state: nothing, since it is the whole deployment.
pub struct Context {
    // Auto traits match the nv backend's, so the public surface is the same on every backend.
    _unshared: PhantomData<*const ()>,
}

// SAFETY: every field is `Send`; the marker exists only to withdraw `Sync`, as nv's pointers do.
unsafe impl Send for Context {}

/// Refuse any deployment but this launch of one: one host, the worker Launch(0), its leader
/// Launch(1).
fn launch_of_one(deployment: Deployment<'_>, who: Launch, operation: &'static str) -> Result<(), Failure> {
    if deployment.hosts().len() != 1 || deployment.workers() != [LAUNCH] || deployment.leader() != LEADER {
        return Err(Failure {
            participant: Participant::Entering(Some(who)),
            operation,
            kind: FailureKind::Backend(BackendFault::Invalid(Invalid::RankOutsideJob)),
        });
    }
    Ok(())
}

/// Enter as the worker of a deployment that names that one worker and its leader. Any other
/// launch set is refused rather than narrowed.
pub fn init(_env: Environment, deployment: Deployment<'_>) -> Result<Context, Failure> {
    launch_of_one(deployment, LAUNCH, "init")?;
    Ok(Context { _unshared: PhantomData })
}

pub fn rank(_cx: &Context) -> u32 {
    ME
}

pub fn size(_cx: &Context) -> u32 {
    1
}

/// One worker has no one to meet, so the phase boundary is already crossed.
pub fn barrier(_cx: &mut Context) {}

/// There is no launch-wide record to discharge, so the outcome is the whole of it.
pub fn done<A>(_cx: &mut Context, outcome: Result<(), Failure<A>>) -> Result<(), Failure<A>> {
    outcome
}

/// Nothing is on the other end: any other address, a `Remote` one included, is outside a launch of
/// one host, and this worker has no route to itself.
fn nowhere(to: Addr) -> Result<(), Error> {
    if to != Addr::Local(ME) {
        return Err(Error::Invalid(Invalid::RankOutsideJob));
    }
    Err(Error::Closed)
}

/// The leader route goes nowhere: the leader is this same process, and nothing moves.
fn nowhere_up() -> Result<(), Error> {
    Err(Error::Closed)
}

pub fn send(_cx: &mut Context, to: Addr, _channel: Channel, _data: &[u8]) -> Result<(), Error> {
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
    workers: &[u32],
    edges: &[Edge],
    bytes: usize,
    _tag: Tag,
) -> Result<(), Error> {
    crate::host::lanes::validate(workers, edges, bytes, size(cx), MAX_FRAME)
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
    receives: bool,
    // Auto traits match the nv backend's, so the public surface is the same on every backend.
    _unshared: PhantomData<&'a *const ()>,
}

// SAFETY: every field is `Send`; the marker exists only to withdraw `Sync`, as nv's pointers do.
unsafe impl Send for Io<'_> {}

/// `concurrent!` given a context: the arms still run, each on its own host thread.
#[doc(hidden)]
pub fn concurrent_io<'env, B, E: Send, const N: usize>(
    _cx: &'env mut Context,
    receive: &'env [Receive<'env>; N],
    body: B,
) -> Result<(), E>
where
    B: for<'scope> FnOnce(&mut crate::host::run::IoArms<'scope, 'env, Io<'env>, E, N>),
{
    let ios = core::array::from_fn(|at| Io {
        receives: Owner::new(receive, at).receives(),
        _unshared: PhantomData,
    });
    crate::host::run::spawn(ios, body)
}

impl Io<'_> {
    pub fn send(&mut self, to: Addr, _channel: Channel, _data: &[u8]) -> Result<(), Error> {
        nowhere(to)
    }

    pub fn lead(&mut self, _tag: Tag, _data: &[u8]) -> Result<(), Error> {
        nowhere_up()
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
        Addr, Deployment, Error, Failure, Frame, Handle, Invalid, Tag,
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
            launch_of_one(deployment, LEADER, "leader::open")?;
            Ok(Leader(PhantomData, Unfrozen(UnsafeCell::new(()))))
        }

        pub fn send(&self, to: u32, _tag: Tag, _data: &[u8]) -> Result<(), Error> {
            nowhere(Addr::Local(to))
        }

        pub fn recv(&self, _out: &mut [u8]) -> Result<Option<Frame>, Error> {
            Ok(None)
        }

        pub fn done<A>(&mut self, outcome: Result<(), Failure<A>>) -> Result<(), Failure<A>> {
            outcome
        }
    }

    pub fn send(_cx: &mut Context, _tag: Tag, _data: &[u8]) -> Result<(), Error> {
        nowhere_up()
    }

    pub fn recv(_cx: &mut Context, _out: &mut [u8]) -> Result<Option<Frame>, Error> {
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

// `backend.md`, Links: "`none` refuses a remote address by name."
#[cfg(test)]
mod tests {
    use super::{Environment, init, send, LAUNCH, LEADER};
    use crate::contract::{Addr, Channel, Deployment, Error, Invalid, Launch, Tag};

    #[test]
    fn a_remote_worker_is_outside_a_launch_of_one() {
        let hosts: [&[Launch]; 1] = [&[LAUNCH]];
        let deployment = Deployment::new(&hosts, 0, LEADER).expect("the launch of one");
        let mut cx = init(Environment, deployment).expect("the one worker enters");
        let to = Addr::Remote { host: 1, rank: 0 };
        assert_eq!(
            send(&mut cx, to, Channel::Message(Tag::new(1)), b"x"),
            Err(Error::Invalid(Invalid::RankOutsideJob))
        );
    }
}

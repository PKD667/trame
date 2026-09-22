// A launch of one: the dummy transport.
//
// This is what a build that links no transport gets: the in-process Python extension and any host
// that wants the model without a fabric. The entry, the clock, the segment and the shared-state
// families all work, because one process alone can answer them.
//
// Traffic is where it is a dummy, deliberately and by its name. A send is accepted and the bytes
// go nowhere; a receive never produces a frame. This is not a refusal dressed as success: there
// is no second participant to fail to reach, and a caller that links `none` has said it is not
// moving anything. Nothing here is a fault to diagnose, so nothing here reports one — but read
// the consequence plainly: **a delivery this backend is asked for does not happen, and a test
// that asserts one passes vacuously here.** A claim about traffic is only checkable on a
// transport, and that is what the transport backends and their launches are for.

use crate::contract::{
    Backend, BackendFault, Channel, Deployment, Edge, Error, Failure, FailureKind, Frame,
    FrameBytes, Invalid, Rank, Tag,
};

pub use crate::cpu::clock;
pub use crate::cpu::run;
pub use crate::cpu::sync;

pub const ID: Backend = Backend::None;

/// No storage holds a frame here, so no length is refused.
pub const MAX_FRAME: FrameBytes = FrameBytes::new(u32::MAX);

pub fn align() -> usize {
    crate::partition::host_align()
}

const ME: Rank = Rank::from_index(0);

/// What the entry is given. Nothing here is a transport's business, because there is no
/// transport; the field exists so a host binding names the same entry whatever it links.
#[derive(Default)]
pub struct Environment;

/// This participant's state. One rank, zero peers.
pub struct Context {
    /// Always `0`, and kept as a field so `rank` reads the same way it does everywhere else.
    rank: Rank,
    /// This rank, as the whole of `hosts` and the whole of `cohort`: alone, a participant is its
    /// own sharing domain's leader and its own cohort.
    alone: [Rank; 1],
}

/// One published segment: the participant's own bytes, because it published all of them.
pub struct Shared(Vec<u8>);

/// Enter the launch.
///
/// A deployment that names participants other than this one is refused rather than narrowed: a
/// launch of one cannot run what it describes, and starting anyway would report a deployment that
/// does not exist.
pub fn init(
    _env: Environment,
    deployment: Deployment<'_>,
    cohort: fn(Rank, &[Rank]) -> u32,
) -> Result<Context, Failure> {
    let refuse = |why| {
        Err(Failure {
            participant: ME,
            operation: "init",
            kind: FailureKind::Backend(BackendFault::Invalid(why)),
        })
    };
    // A leader would be a second process, and there is none.
    if deployment.workers() != [ME] || deployment.leaders().is_some() {
        return refuse(Invalid::RankOutsideJob);
    }
    // The rule still runs: it is the caller's statement about sharing domains, and a rule that
    // does not name this rank's domain is wrong here for the same reason it is wrong anywhere.
    if cohort(ME, &[ME]) != 0 {
        return refuse(Invalid::RankOutsideJob);
    }
    Ok(Context {
        rank: ME,
        alone: [ME],
    })
}

pub fn rank(cx: &Context) -> Rank {
    cx.rank
}

pub fn size(_cx: &Context) -> u32 {
    1
}

pub fn hosts(cx: &Context) -> &[Rank] {
    &cx.alone
}

pub fn cohort(cx: &Context) -> &[Rank] {
    &cx.alone
}

/// Report the outcome. There is no launch-wide record to discharge, so the outcome is the whole
/// of it and travels back to the caller that owns the exit.
pub fn done<A>(_cx: &mut Context, outcome: Result<(), Failure<A>>) -> Result<(), Failure<A>> {
    outcome
}

/// Accept the frame and discard it.
///
/// Accepted, because a send's success means the backend took the bytes, and this one takes them
/// the way `/dev/null` takes them. Not a loopback: a participant addressing itself would have its
/// own frame handed back as if a deployment it does not have had worked.
pub fn send(
    _cx: &mut Context,
    _to: Rank,
    _channel: Channel,
    _data: &[u8],
) -> Result<(), Error> {
    Ok(())
}

/// Nothing ever arrives, because nothing was ever kept.
pub fn recv(_cx: &mut Context, _out: &mut [u8]) -> Result<Option<Frame>, Error> {
    Ok(None)
}

/// Every accepted send already released everything it held, which was nothing.
pub fn flush(_cx: &mut Context) -> Result<(), Error> {
    Ok(())
}

/// Take the lane table for this load.
///
/// A table naming any edge is refused: an edge is traffic between two participants and there is
/// only one. An empty table over this one rank is accepted, so a load that places everything
/// locally declares its geometry here like any other.
pub fn reshape(
    _cx: &mut Context,
    workers: &[Rank],
    edges: &[Edge],
    _bytes: FrameBytes,
    _tag: Tag,
) -> Result<(), Error> {
    if workers.iter().any(|&w| w != ME) {
        return Err(Error::Invalid(Invalid::RankOutsideJob));
    }
    if !edges.is_empty() {
        return Err(Error::Invalid(Invalid::UnsupportedGeometry));
    }
    Ok(())
}

/// Nothing was taken, so there is nothing to discharge.
pub fn release(_cx: &mut Context) -> Result<(), Error> {
    Ok(())
}

/// Publish this participant's slice. It is the whole segment, so publication is the move itself
/// and there is no collective to reach.
pub fn share(cx: &mut Context, mine: &[u8], total: usize) -> Result<Shared, Error> {
    let range = crate::partition::slice_of(hosts(cx), cohort(cx), cx.rank, total)
        .map_err(Error::Invalid)?;
    if mine.len() != range.length || range.length != total {
        return Err(Error::Invalid(Invalid::BadShareLength));
    }
    Ok(Shared(mine.to_vec()))
}

pub fn bytes(segment: &Shared) -> &[u8] {
    &segment.0
}

/// Nothing was mapped, so retiring the handle is dropping it.
pub fn unshare(_cx: &mut Context, _segment: Shared) -> Result<(), (Shared, Error)> {
    Ok(())
}

/// The leader route of a launch that has one participant to lead nobody with.
///
/// The route opens: a lone process leads itself. Its traffic is the dummy's, like `send`'s.
pub mod leader {
    use super::{Context, ME};
    use crate::contract::{
        BackendFault, Deployment, Error, Failure, FailureKind, Frame, FrameBytes, Invalid, Rank,
        Tag,
    };

    /// The leader's end: a route over an empty worker set.
    pub struct Leader(());

    impl Leader {
        /// Open the route. Only the lone process itself may be named, because nobody else is here.
        pub fn open(
            _env: super::Environment,
            deployment: Deployment<'_>,
        ) -> Result<Leader, Failure> {
            if deployment.workers() != [ME] || deployment.leaders().is_some() {
                return Err(Failure {
                    participant: ME,
                    operation: "leader::open",
                    kind: FailureKind::Backend(BackendFault::Invalid(Invalid::RankOutsideJob)),
                });
            }
            Ok(Leader(()))
        }

        pub fn send(&self, _to: Rank, _tag: Tag, _data: &[u8]) -> Result<(), Error> {
            Ok(())
        }

        pub fn recv(&self, _out: &mut [u8]) -> Result<Option<Frame>, Error> {
            Ok(None)
        }
    }

    /// The worker's end: a lone process leads itself, and what it says to itself goes nowhere.
    pub fn send(_cx: &mut Context, _tag: Tag, _data: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    pub fn recv(_cx: &mut Context, _out: &mut [u8]) -> Result<Option<(Tag, FrameBytes)>, Error> {
        Ok(None)
    }
}

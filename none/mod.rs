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
    Channel, Declarations, Deployment, Edge, Error, Failure, Frame, LANE_UNAVAILABLE,
    PUBLICATION_COLLECTIVE, RELEASE_LOCAL, Rank, Reading, Scopes, Tag, Wait,
};

pub use crate::cpu::clock;
pub use crate::cpu::sync;
pub use crate::cpu::run;

/// Backend identifier stored in `LOAD` records.
pub const ID: u8 = 3;

pub const DECLARATIONS: Declarations = Declarations {
    // One process: an atomic orders that process's threads, which is exactly the participant
    // scope. There is no domain and no system beyond it to order.
    atomic_scopes: Scopes {
        participant: true,
        domain: false,
        system: false,
    },
    // Not "unreliable": absent. There is no lane route here at all, and saying so is what stops
    // a caller reading the missing `Full` on it as room.
    lane_reliability: LANE_UNAVAILABLE,
    // A send never waits, because it never gets as far as needing room: it is refused outright.
    waiting_message: 0,
    waiting_lane: 0,
    // Capacity cannot be observed because there is no queue to observe. A caller that depends on
    // flow control requires `backpressure`, which this backend does not declare.
    pressure_message: false,
    pressure_lane: false,
    release: RELEASE_LOCAL,
    publication: PUBLICATION_COLLECTIVE,
    priority: true,
    resident: false,
    max_frame: u32::MAX,
    tag_limit: u32::MAX,
    lowering: "scalar",
};

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
    let refuse = |code: u32| {
        Err(Failure {
            participant: 0,
            operation: "init",
            code,
        })
    };
    if deployment.refuse().is_some() {
        return refuse(1);
    }
    if deployment
        .workers
        .iter()
        .chain(deployment.leaders)
        .any(|&rank| rank != 0)
    {
        return refuse(2);
    }
    // The rule still runs: it is the caller's statement about sharing domains, and a rule that
    // does not name this rank's domain is wrong here for the same reason it is wrong anywhere.
    if cohort(0, &[0]) != 0 {
        return refuse(3);
    }
    Ok(Context {
        rank: 0,
        alone: [0],
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
pub fn done(_cx: &mut Context, outcome: Result<(), Failure>) -> Result<(), Failure> {
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
    _wait: Wait,
) -> Result<(), Error> {
    Ok(())
}

/// Nothing ever arrives, because nothing was ever kept.
pub fn recv(_cx: &mut Context, _out: &mut [u8], _wait: Wait) -> Result<Option<Frame>, Error> {
    Ok(None)
}

/// Every accepted send already released everything it held, which was nothing.
pub fn flush(_cx: &mut Context, _wait: Wait) -> Result<(), Error> {
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
    _bytes: u32,
    _tag: Tag,
) -> Result<(), Error> {
    if workers.iter().any(|&w| w != 0) {
        return Err(Error::Invalid { code: 1 });
    }
    if !edges.is_empty() {
        return Err(Error::Invalid { code: 2 });
    }
    Ok(())
}

/// Nothing was taken, so there is nothing to discharge.
pub fn release(_cx: &mut Context) -> Result<(), Error> {
    Ok(())
}

/// The participant's share of a segment, by the shared pure rule. Alone, the share is all of it.
pub fn slice(cx: &Context, rank: Rank, total: usize) -> (usize, usize) {
    crate::partition::slice_of(hosts(cx), cohort(cx), rank, total, 1)
}

/// Publish this participant's slice. It is the whole segment, so publication is the move itself
/// and there is no collective to reach.
pub fn share(cx: &mut Context, mine: &[u8], total: usize) -> Result<Shared, Error> {
    let (_, length) = slice(cx, cx.rank, total);
    if mine.len() != length || length != total {
        return Err(Error::Invalid { code: 3 });
    }
    Ok(Shared(mine.to_vec()))
}

pub fn bytes(segment: &Shared) -> &[u8] {
    &segment.0
}

/// Nothing was mapped, so nothing has to be unmapped and the handle stays valid.
pub fn unshare(_cx: &mut Context, _segment: &mut Shared) -> Result<(), Error> {
    Ok(())
}

/// The host's monotonic clock, with this process as its comparison domain.
pub fn reading(_cx: &Context) -> Result<Reading, Error> {
    Ok(clock::reading())
}

/// The leader route of a launch that has one participant to lead nobody with.
///
/// The route opens: a lone process leads itself, and a leader with an empty worker set is the
/// honest shape of a deployment with nothing deployed. Its traffic is the dummy's, like `send`'s.
pub mod leader {
    use super::Context;
    use crate::contract::{Deployment, Error, Failure, Frame, Rank, Tag, Wait};

    /// The leader's end: a route over an empty worker set.
    pub struct Leader(());

    impl Leader {
        /// Open the route. A deployment naming workers is refused, because they are not here.
        pub fn open(
            _env: super::Environment,
            deployment: Deployment<'_>,
        ) -> Result<Leader, Failure> {
            if deployment
                .workers
                .iter()
                .chain(deployment.leaders)
                .any(|&rank| rank != 0)
            {
                return Err(Failure {
                    participant: 0,
                    operation: "leader::open",
                    code: 1,
                });
            }
            Ok(Leader(()))
        }

        pub fn send(&self, _to: Rank, _tag: Tag, _data: &[u8], _wait: Wait) -> Result<(), Error> {
            Ok(())
        }

        pub fn recv(&self, _out: &mut [u8], _wait: Wait) -> Result<Option<Frame>, Error> {
            Ok(None)
        }
    }

    /// The worker's end: a lone process leads itself, and what it says to itself goes nowhere.
    pub fn send(_cx: &mut Context, _tag: Tag, _data: &[u8], _wait: Wait) -> Result<(), Error> {
        Ok(())
    }

    pub fn recv(
        _cx: &mut Context,
        _out: &mut [u8],
        _wait: Wait,
    ) -> Result<Option<(Tag, u32)>, Error> {
        Ok(None)
    }
}

// A launch of one: the in-process transport.
//
// This is what a build that links no transport gets: the in-process Python extension and any host
// that wants the model without a fabric. The entry, the clock, the segment and the shared-state
// families all work, because one process alone can answer them.
//
// Traffic is real, but it never leaves the process. One process holds the worker, Launch(0), and
// optionally its leader, Launch(1), on a second host thread, and moves owned frames between them
// over three bounded FIFOs. An accepted frame is owned exactly once and delivered exactly once;
// a refusal consumes nothing. Nothing here crosses a process boundary, which is what makes it the
// dummy: every route is a queue in this address space.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::contract::{
    Backend, BackendFault, Channel, Deployment, Edge, Error, Failure, FailureKind, Frame, Invalid,
    Launch, Participant, Rank, Tag,
};

pub use crate::cpu::clock;
pub use crate::cpu::run;
pub use crate::cpu::sync;

pub const ID: Backend = Backend::None;

/// The contract's floor: a 64 KiB log block behind an eight-byte batch header. Routes here copy
/// into owned bytes, so nothing narrows it.
pub const MAX_FRAME: usize = 65_544;

pub fn align() -> usize {
    crate::partition::host_align()
}

const ME: Rank = Rank::from_index(0);

/// The one process's worker launch rank. A launch of one worker is transport rank zero, and the
/// deployment is compared against it rather than against a contract rank: the two spaces coincide
/// here and nothing crosses between them.
const LAUNCH: Launch = Launch::new(0);

/// The optional leader's launch rank. It is a second host thread of this same process, and it is
/// never a worker: the deployment's uniform rule says a launch is one or the other.
const LEADER: Launch = Launch::new(1);

/// One frame on a route: what was sent and who sent it. Owned, because an accepted frame belongs
/// to the backend until its receiver takes it.
struct Held {
    source: Rank,
    tag: Tag,
    data: Vec<u8>,
}

/// The three routes of the one job these entries name. The job owns them, and the entries that
/// share it reach them through the `Environment` they were handed, so two jobs in one process do
/// not touch each other's frames.
#[derive(Default)]
struct Routes {
    /// A worker's frames to itself: the `Message` and `Lane` channels addressed to Rank 0.
    worker: VecDeque<Held>,
    /// A worker's frames to its leader.
    to_leader: VecDeque<Held>,
    /// The leader's frames to its worker.
    to_worker: VecDeque<Held>,
}

/// The depth of every route. Bounded so a sender can be refused: `Full` is reachable, and a
/// refusal that consumes nothing is the contract these routes keep.
const DEPTH: usize = 64;

/// The lane geometry a load declared: the tag lane frames carry, the declared edges, and the byte
/// capacity of a lane frame. A lane send is measured against `bytes`, not `MAX_FRAME`, because the
/// declaration is what the load asked the route to hold.
struct Lane {
    tag: Tag,
    edges: Vec<Edge>,
    bytes: usize,
}

/// Accept one owned frame onto `queue`, or refuse it before anything is kept. The length is
/// checked against `limit` first, then capacity, so a frame that is both too large and arriving
/// full is reported as the caller's own mistake.
fn enqueue(
    queue: &mut VecDeque<Held>,
    source: Rank,
    tag: Tag,
    data: &[u8],
    limit: usize,
) -> Result<(), Error> {
    if data.len() > limit {
        return Err(Error::TooLarge { limit });
    }
    if queue.len() >= DEPTH {
        return Err(Error::Full);
    }
    queue.push_back(Held {
        source,
        tag,
        data: data.to_vec(),
    });
    Ok(())
}

/// Take the front frame into `out`, or refuse without consuming it. The length is checked first
/// and the pop happens only once the frame fits, so a short buffer leaves the frame where it was.
/// Both receives take from here, so their front check cannot drift apart.
fn take(queue: &mut VecDeque<Held>, out: &mut [u8]) -> Result<Option<Held>, Error> {
    let Some(held) = queue.front() else {
        return Ok(None);
    };
    if held.data.len() > out.len() {
        return Err(Error::TooSmall {
            needed: held.data.len(),
        });
    }
    let held = queue.pop_front().expect("the front was just read");
    out[..held.data.len()].copy_from_slice(&held.data);
    Ok(Some(held))
}

/// Take the front frame into `out`, together with its source and tag.
fn dequeue(queue: &mut VecDeque<Held>, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    Ok(take(queue, out)?.map(|held| Frame::new(held.source, held.tag, held.data.len())))
}

/// Take the front frame into `out` and report its tag and length. A leader route's frame carries
/// no source for the leader to read, for the same reason a worker's send carries no destination.
fn dequeue_tagged(
    queue: &mut VecDeque<Held>,
    out: &mut [u8],
) -> Result<Option<(Tag, usize)>, Error> {
    Ok(take(queue, out)?.map(|held| (held.tag, held.data.len())))
}

/// What the entry is given. Nothing here is a transport's business, because there is no
/// transport; the field exists so a host binding names the same entry whatever it links.
///
/// The routes belong to the job, not to the process. `default` is a new job with empty routes; a
/// clone is the same job, which is how `init` and `Leader::open` reach one set of queues without
/// a process-global that another job's entry would clear.
#[derive(Clone, Default)]
pub struct Environment {
    routes: Arc<Mutex<Routes>>,
}

/// This participant's state. One worker rank, and the tag a lane frame carries once a load
/// declares one.
pub struct Context {
    /// Always `0`, and kept as a field so `rank` reads the same way it does everywhere else.
    rank: Rank,
    /// This rank, as the whole of `hosts` and the whole of `cohort`: alone, a participant is its
    /// own sharing domain's leader and its own cohort.
    alone: [Rank; 1],
    /// The routes of the job this participant entered, shared with its leader through the
    /// `Environment` both were handed.
    routes: Arc<Mutex<Routes>>,
    /// This load's declared lane geometry, or `None` before one declares it.
    lane: Option<Lane>,
    /// Whether this process's deployment named a leader for this worker.
    led: bool,
}

/// One published segment: the participant's own bytes, because it published all of them.
pub struct Shared(Vec<u8>);

/// Enter the launch.
///
/// This process is the worker, Launch(0), of a deployment that names that one worker and at most
/// its leader, Launch(1). Any other launch set is refused rather than narrowed: a launch of one
/// cannot run what it describes, and starting anyway would report a deployment that does not
/// exist.
pub fn init(
    env: Environment,
    deployment: Deployment<'_>,
    cohort: fn(Rank, &[Rank]) -> u32,
) -> Result<Context, Failure> {
    let refuse = |why| {
        Err(Failure {
            participant: Participant::Entering(Some(LAUNCH)),
            operation: "init",
            kind: FailureKind::Backend(BackendFault::Invalid(why)),
        })
    };
    let led = match deployment.leaders() {
        None => false,
        Some(leaders) if leaders == [LEADER] => true,
        Some(_) => return refuse(Invalid::RankOutsideJob),
    };
    if deployment.workers() != [LAUNCH] {
        return refuse(Invalid::RankOutsideJob);
    }
    // The rule still runs: it is the caller's statement about sharing domains, and a rule that
    // does not name this rank's domain is wrong here for the same reason it is wrong anywhere.
    if cohort(ME, &[ME]) != 0 {
        return refuse(Invalid::RankOutsideJob);
    }
    // Entering names the job through its `Environment` and clears nothing: a clone of this
    // environment is the same job, and a second job is a second `Environment`.
    Ok(Context {
        rank: ME,
        alone: [ME],
        routes: env.routes,
        lane: None,
        led,
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

fn cohort(cx: &Context) -> &[Rank] {
    &cx.alone
}

/// Report the outcome. There is no launch-wide record to discharge, so the outcome is the whole
/// of it and travels back to the caller that owns the exit.
pub fn done<A>(_cx: &mut Context, outcome: Result<(), Failure<A>>) -> Result<(), Failure<A>> {
    outcome
}

/// Send one frame to this worker's own rank. A lane frame takes the tag and capacity `reshape`
/// declared and is refused when the load gave the destination no edge; a message frame carries
/// its own tag. Another rank, or a lane before a load declares one, is refused by name.
pub fn send(cx: &mut Context, to: Rank, channel: Channel, data: &[u8]) -> Result<(), Error> {
    if to != cx.rank {
        return Err(Error::Invalid(Invalid::RankOutsideJob));
    }
    let (tag, limit) = match channel {
        Channel::Message(tag) => (tag, MAX_FRAME),
        Channel::Lane => {
            let lane = cx
                .lane
                .as_ref()
                .ok_or(Error::Invalid(Invalid::LaneNotConfigured))?;
            if !lane.edges.iter().any(|edge| edge.destination() == to) {
                return Err(Error::Invalid(Invalid::NoLane));
            }
            (lane.tag, lane.bytes)
        }
    };
    let source = cx.rank;
    let mut routes = cx.routes.lock().expect("the route table");
    enqueue(&mut routes.worker, source, tag, data, limit)
}

/// Take the next frame this worker sent itself. A short buffer refuses without consuming the
/// frame; an empty route gives `None`.
pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    let mut routes = cx.routes.lock().expect("the route table");
    dequeue(&mut routes.worker, out)
}

/// Every accepted send was owned by a route and stays there until its receiver takes it, so there
/// is no buffer to release.
pub fn flush(_cx: &mut Context) -> Result<(), Error> {
    Ok(())
}

/// Take the lane table for this load.
///
/// There is one worker, so an edge must be this rank on both ends. The declaration's geometry —
/// the edges and the byte capacity — is recorded, and a lane send is measured against the
/// recorded capacity rather than `MAX_FRAME`.
pub fn reshape(
    cx: &mut Context,
    workers: &[Rank],
    edges: &[Edge],
    bytes: usize,
    tag: Tag,
) -> Result<(), Error> {
    if workers.iter().any(|&w| w != ME) {
        return Err(Error::Invalid(Invalid::RankOutsideJob));
    }
    // An edge is traffic between two participants. There is one rank here, so a declared edge has
    // to be this rank on both ends.
    if edges
        .iter()
        .any(|edge| edge.source() != ME || edge.destination() != ME)
    {
        return Err(Error::Invalid(Invalid::EdgeOutsideWorkers));
    }
    cx.lane = Some(Lane {
        tag,
        edges: edges.to_vec(),
        bytes,
    });
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

/// The leader route of a launch that has an optional second host thread. The leader is Launch(1)
/// and holds no contract rank; the worker is Launch(0). A deployment that names no leader has no
/// route to open, and a worker without one has no route to send on.
pub mod leader {
    use super::{Context, LAUNCH, LEADER, ME, Routes, dequeue, dequeue_tagged, enqueue};
    use std::sync::{Arc, Mutex};
    use crate::contract::{
        BackendFault, Deployment, Error, Failure, FailureKind, Frame, Invalid, Participant, Rank,
        Tag,
    };

    /// The leader's end: the second host thread of this process, holding no rank.
    pub struct Leader(Arc<Mutex<Routes>>);

    impl Leader {
        /// Open the route. Only a deployment naming the worker Launch(0) and this leader Launch(1)
        /// is this process's to open; a deployment with no leader has no route, and any other set
        /// is a process outside the job.
        pub fn open(
            env: super::Environment,
            deployment: Deployment<'_>,
        ) -> Result<Leader, Failure> {
            let refuse = |why| {
                Err(Failure {
                    participant: Participant::Entering(Some(LEADER)),
                    operation: "leader::open",
                    kind: FailureKind::Backend(BackendFault::Invalid(why)),
                })
            };
            match deployment.leaders() {
                None => return refuse(Invalid::NoLeader),
                Some(leaders) if leaders == [LEADER] => {}
                Some(_) => return refuse(Invalid::RankOutsideJob),
            }
            if deployment.workers() != [LAUNCH] {
                return refuse(Invalid::RankOutsideJob);
            }
            // Opening names the job through its `Environment` and clears nothing, for the same
            // reason `init` does not: a clone is the same job and a second job is a second
            // `Environment`.
            Ok(Leader(env.routes))
        }

        /// Send one frame to the worker, named by contract rank. Another rank is outside this
        /// launch of one.
        pub fn send(&self, to: Rank, tag: Tag, data: &[u8]) -> Result<(), Error> {
            if to != ME {
                return Err(Error::Invalid(Invalid::RankOutsideJob));
            }
            let mut routes = self.0.lock().expect("the route table");
            enqueue(&mut routes.to_worker, ME, tag, data, super::MAX_FRAME)
        }

        /// Take the next frame a worker sent up the route.
        pub fn recv(&self, out: &mut [u8]) -> Result<Option<Frame>, Error> {
            let mut routes = self.0.lock().expect("the route table");
            dequeue(&mut routes.to_leader, out)
        }
    }

    /// A worker's send to its leader. No leader in the deployment is no route.
    pub fn send(cx: &mut Context, tag: Tag, data: &[u8]) -> Result<(), Error> {
        if !cx.led {
            return Err(Error::Invalid(Invalid::NoLeader));
        }
        let source = cx.rank;
        let mut routes = cx.routes.lock().expect("the route table");
        enqueue(&mut routes.to_leader, source, tag, data, super::MAX_FRAME)
    }

    /// A worker's receive from its leader, as `(tag, length)`.
    pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<(Tag, usize)>, Error> {
        if !cx.led {
            return Err(Error::Invalid(Invalid::NoLeader));
        }
        let mut routes = cx.routes.lock().expect("the route table");
        dequeue_tagged(&mut routes.to_worker, out)
    }
}

//! The leader's route.
//!
//! §4 says what a leader is: a host process that holds no rank, is absent from `size`, `rank`,
//! `hosts` and `cohort`, and reaches its workers over a route of its own. §4 also leaves the
//! realization to the backend. This is this backend's, and the shape of it is the shape of the
//! statement: the leader is a **different kind of object** from a worker, not one more rank of the
//! worker population with unused fields, and what connects them is a **route of its own** rather
//! than the message route with a filter on it.
//!
//! Concretely, the job is split into a worker group and a leader group and an inter-communicator is
//! built between them. Three things follow, and each is the contract rather than a convenience:
//!
//! - The worker group *is* the contract's participant set. A worker's rank in it is its contract
//!   rank, `size` is how many there are, and the leader is not in it — so the leader is absent from
//!   `size`, `rank`, `hosts` and `cohort` by construction rather than by remembering to exclude it.
//! - The remote group's ranks *are* those same contract ranks, so `Frame::source` on the leader's
//!   receive is a worker's contract rank directly and there is no renumber map to get wrong.
//! - Each end of the bridge has a known population and neither needs to name the other: a worker's
//!   send has no destination because the leader end is one rank, and the leader's receive has no
//!   source filter because every frame on it came from a worker. That is why §4 gives the worker no
//!   destination and the leader no source, and it is not a simplification — it is what a bridge
//!   between two known groups buys.
//!
//! The one thing here that is not a wrapper call is the creation itself: rsmpi has
//! `InterCommunicator` and no `create`, because its wrapper shape does not fit a call whose two
//! ends pass different arguments. So the raw interface is called and the code mapped, the same way
//! and for the same reason as `MPI_Bsend` in `p2p`.

use std::sync::Mutex;

use mpi::environment::Universe;
use mpi::point_to_point::Message;
use mpi::raw::traits::AsRaw;
use mpi::topology::{Color, Communicator, InterCommunicator, SimpleCommunicator};

use super::context::{Context, Environment};
use super::p2p::{self, send_on, take};
use crate::contract::{Deployment, Error, Failure, Frame, Rank, Tag, Wait};

/// The tag that labels the bridge's creation. It names the collective that builds the
/// inter-communicator and is consumed by MPI's context-id machinery, so it never reaches a message
/// and cannot collide with a caller's tag.
pub(crate) const BRIDGE_TAG: i32 = 42;

/// The tag for one leader's bridge.
///
/// One tag per pair rather than one for all of them, because several bridges are built in the same
/// collective and the tag is part of what MPI matches on. Two leaders sharing a tag would be two
/// bridges that MPI is entitled to pair the other way round, and the symptom would be a leader
/// bridged to another leader's workers.
pub(crate) fn bridge_tag(index: usize) -> i32 {
    BRIDGE_TAG + index as i32
}

/// Build the bridge between the calling rank's own group and the leader of the other.
///
/// # Safety-adjacent contract
///
/// Collective over `local`, and both groups must call it with the same bridge communicator and
/// tag. The two ends differ in one argument: each names the *other* group's leader. That asymmetry
/// is the whole reason rsmpi has no wrapper for it.
pub(crate) fn bridge(
    local: &SimpleCommunicator,
    world: &SimpleCommunicator,
    remote_leader: Rank,
    index: usize,
) -> Option<InterCommunicator> {
    let mut handle = unsafe { mpi::ffi::RSMPI_COMM_NULL };
    let code = unsafe {
        mpi::ffi::MPI_Intercomm_create(
            local.as_raw(),
            0,
            world.as_raw(),
            remote_leader as i32,
            bridge_tag(index),
            &mut handle,
        )
    };
    if code != 0 {
        return None;
    }
    unsafe { InterCommunicator::try_from_raw(handle) }
}

/// A leader. Owned by the host process, one per job, and not a participant.
///
/// It has no `Context`, no rank and no cohort, and that is the point: there is nothing here for a
/// caller to mistake for a participant's state. It holds its own MPI lifetime, because a leader
/// enters MPI on its own account — `init` is the worker's entry and a leader that called it would
/// be claiming a rank it does not have.
///
/// The fields are declared in the order they must be released, because that is the order Rust
/// drops them: the bridge disconnects, the group frees, and MPI finalises last. `MPI_Comm_disconnect`
/// is collective over both groups, so a leader that released its bridge at an arbitrary moment —
/// whenever a value happened to die — would be choosing a moment its workers did not choose.
pub struct Leader {
    inter: InterCommunicator,
    /// This leader's workers, as contract ranks, in the order the bridge addresses them.
    ///
    /// The remote group's ranks are *not* contract ranks once there is more than one leader: a
    /// leader's remote group is only the workers it serves, so remote rank 0 is that leader's first
    /// worker and not contract rank 0. This is the map, and it is the composition of the two lists
    /// the deployment already carries rather than a second rule — computed here once, in the one
    /// place that has both the declaration and the remote order.
    mine: Box<[Rank]>,
    /// The leader group, kept alive for as long as the bridge is: dropping it would disconnect a
    /// communicator the bridge was built against.
    group: SimpleCommunicator,
    universe: Universe,
    /// A frame that was matched and did not fit the caller's buffer, for the same reason a
    /// participant holds one: MPI has no un-probe, and a refusal must not consume the frame the
    /// caller was told to grow for.
    held: Mutex<Option<Message>>,
    /// The attached buffer size the entry declared, which bounds a polling send. A field rather
    /// than a process global: it is set on this process's universe and the leader is the only
    /// thing that reads it.
    bsend_bytes: usize,
}

impl Leader {
    /// Open the route. This is the leader process's MPI entry.
    ///
    /// A leader serves the workers the declaration assigns to it and no others, so it builds one
    /// bridge to that group. Which group that is comes from the declaration alone: the same colour
    /// arithmetic the workers run, which is why the two ends cannot disagree about the pairing.
    pub fn open(env: Environment, deployment: Deployment<'_>) -> Result<Leader, Failure> {
        if deployment.refuse().is_some() {
            return Err(Failure {
                participant: 0,
                operation: "leader::open",
                code: 8,
            });
        }
        let (mut universe, threading) = mpi::initialize_with_threading(mpi::Threading::Multiple)
            .ok_or(Failure {
                participant: 0,
                operation: "leader::open",
                code: 2,
            })?;
        if threading != mpi::Threading::Multiple {
            return Err(Failure {
                participant: 0,
                operation: "leader::open",
                code: 3,
            });
        }
        universe.set_buffer_size(env.bsend_bytes);
        let job = universe.world();
        super::context::return_errors(&job);
        let me = u32::try_from(job.rank()).map_err(|_| Failure {
            participant: 0,
            operation: "leader::open",
            code: 9,
        })?;

        // The odd half of the pair the workers' even colours name.
        let index = deployment.leader_index(me).ok_or(Failure {
            participant: me,
            operation: "leader::open",
            code: 1,
        })?;
        let group = job
            .split_by_color_with_key(Color::with_value((index * 2 + 1) as i32), 0)
            .ok_or(Failure {
                participant: me,
                operation: "leader::open",
                code: 4,
            })?;
        if group.size() != 1 {
            // One leader per colour, by construction: two leaders sharing a colour would be two
            // processes claiming one worker group, which no declaration can mean.
            return Err(Failure {
                participant: me,
                operation: "leader::open",
                code: 10,
            });
        }

        // This leader's workers, by contract rank and in the remote group's order. The workers'
        // key was their contract rank, so the bridge addresses them in ascending contract rank and
        // this list is the map between the two.
        let mine: Box<[Rank]> = (0..deployment.workers.len() as Rank)
            .filter(|&contract| deployment.leader_of(contract) == Some(me))
            .collect();
        let first = *mine.first().ok_or(Failure {
            participant: me,
            operation: "leader::open",
            code: 6,
        })?;
        let remote_leader = deployment.workers[first as usize];
        let inter = bridge(&group, &job, remote_leader, index).ok_or(Failure {
            participant: me,
            operation: "leader::open",
            code: 7,
        })?;

        Ok(Leader {
            inter,
            mine,
            group,
            universe,
            held: Mutex::new(None),
            bsend_bytes: env.bsend_bytes,
        })
    }

    /// Send one frame to one worker, named by contract rank.
    ///
    /// A worker this leader does not serve is refused rather than clamped: the bridge has no rank
    /// for it, and a frame delivered to whichever worker sits at the end of the remote group is a
    /// fault neither end can see.
    pub fn send(&self, to: Rank, tag: Tag, data: &[u8], wait: Wait) -> Result<(), Error> {
        let remote = self
            .mine
            .iter()
            .position(|&contract| contract == to)
            .ok_or(Error::Invalid { code: 6 })?;
        p2p::send_on(
            &self.inter,
            self.bsend_bytes,
            remote as Rank,
            tag,
            data,
            wait,
        )
    }

    /// Take the next frame from any worker.
    ///
    /// Every frame on this route came from a worker, so there is no source to filter and none is
    /// taken. `Frame::source` is the sending worker's contract rank, because the remote group's
    /// ranks are the contract ranks.
    pub fn recv(&self, out: &mut [u8], wait: Wait) -> Result<Option<Frame>, Error> {
        let mut held = self.held.lock().map_err(|_| self.failure("leader::recv"))?;
        let Some(mut frame) = p2p::take(&self.inter, &mut held, out, wait)? else {
            return Ok(None);
        };
        // The remote rank is a position in this leader's group, not a contract rank: see `mine`.
        frame.source = *self
            .mine
            .get(frame.source as usize)
            .ok_or(Error::Invalid { code: 7 })?;
        Ok(Some(frame))
    }

    fn failure(&self, operation: &'static str) -> Error {
        Error::Failed(Failure {
            participant: 0,
            operation,
            code: 0,
        })
    }
}

/// A worker's send: one frame to this job's leader.
///
/// The destination is not a parameter and is not the caller's to choose. A bridge has a known
/// population at each end, so the leader end is one rank and this is the one place that rank is
/// written. §4 gives the worker end no destination, and this is what that costs at the point of
/// use: nothing.
pub fn send(cx: &mut Context, tag: Tag, data: &[u8], wait: Wait) -> Result<(), Error> {
    let bytes = cx.bsend_bytes();
    let (comm, _) = cx.leader_split().ok_or(Error::Invalid { code: 5 })?;
    send_on(comm, bytes, LEADER, tag, data, wait)
}

/// A worker's receive: the leader's next frame, as `(tag, length)`.
///
/// No source, for the reason a worker's send has no destination: every frame on this route came
/// from the leader. A waiting call returns a frame or an error and never `None` — "nothing
/// arrived" is a poll's answer, and a bridge has no second source whose silence could have been
/// mistaken for it.
pub fn recv(cx: &mut Context, out: &mut [u8], wait: Wait) -> Result<Option<(Tag, u32)>, Error> {
    let Some((comm, held)) = cx.leader_split() else {
        return Err(Error::Invalid { code: 5 });
    };
    let Some(frame) = take(comm, held, out, wait)? else {
        return Ok(None);
    };
    Ok(Some((frame.tag, frame.len)))
}

/// The leader's rank *in the worker group's view of the bridge*: the leader end is one rank, and
/// this is its number. Not a contract rank, and not the launch's numbering either — it is the
/// remote rank on this communicator, which is why nothing outside this module names it.
const LEADER: Rank = 0;

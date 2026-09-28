//! The leader's route.
//!
//! §4 says what a leader is: a host process that holds no rank, is absent from `size`, `rank`,
//! `hosts` and `cohort`, and reaches its workers over a route of its own. §4 also leaves the
//! realization to the backend. This is this backend's, and the shape of it is the shape of the
//! statement: the leader is a **different kind of object** from a worker, not one more rank of the
//! worker population with unused fields, and what connects them is a **route of its own** rather
//! than the message route with a filter on it.
//!
//! Concretely, the job is split into the worker world and individual leader groups. Each worker
//! subgroup builds a bridge to its leader. Three things follow:
//!
//! - The worker group *is* the contract's participant set. A worker's rank in it is its contract
//!   rank, `size` is how many there are, and the leader is not in it — so the leader is absent from
//!   `size`, `rank`, `hosts` and `cohort` by construction rather than by remembering to exclude it.
//! - The bridge's remote ranks are subgroup positions, translated by `mine` to contract ranks
//!   on the leader's receive.
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

use std::marker::PhantomData;
use std::num::NonZeroU64;

use mpi::environment::Universe;
use mpi::raw::traits::AsRaw;
use mpi::topology::{Color, Communicator, InterCommunicator, SimpleCommunicator};

use super::context::{Context, Environment, Io, enter, refused};
use super::p2p::{self, send_on, take};
use crate::contract::{
    BackendFault, Deployment, Error, Failure, Frame, Handle, Invalid, Launch,
    Participant, Rank, Tag,
};
use crate::invoke::Owner;

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
    remote_leader: Launch,
    index: usize,
) -> Option<InterCommunicator> {
    let mut handle = unsafe { mpi::ffi::RSMPI_COMM_NULL };
    let code = unsafe {
        mpi::ffi::MPI_Intercomm_create(
            local.as_raw(),
            0,
            world.as_raw(),
            remote_leader.get() as i32,
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
    /// Who this leader's failures are observed by: its launch rank, since it has no contract rank.
    me: Participant,
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
    _group: SimpleCommunicator,
    _universe: Universe,
    // Auto traits match the nv backend's, so the public surface is the same on every backend.
    _local: PhantomData<*const ()>,
}

impl Leader {
    /// Open the route. This is the leader process's MPI entry.
    ///
    /// A leader serves the workers the declaration assigns to it and no others, so it builds one
    /// bridge to their worker subgroup. Both ends derive the subgroup from the declaration.
    pub fn open(env: Environment, deployment: Deployment<'_>) -> Result<Leader, Failure> {
        let (universe, job, me, _) = enter(&env, "leader::open")?;
        let unentered = Participant::Entering(Some(me));
        let wrong = |why| refused(unentered, "leader::open", BackendFault::Invalid(why));
        if deployment.leaders().is_none() {
            return Err(wrong(Invalid::NoLeader));
        }

        // The same job split as `init`: workers all use colour zero, each leader gets its own
        // group. A process the declaration does not name still takes part in this split.
        let index = deployment.leader_index(me);
        let color = index.map_or(i32::MAX, |index| index as i32 + 1);
        let group = job
            .split_by_color_with_key(Color::with_value(color), 0)
            .ok_or(refused(unentered, "leader::open", BackendFault::Transport))?;
        let index = index.ok_or(wrong(Invalid::WrongLeader))?;

        // This leader's workers, by contract rank and in the remote group's order. The workers'
        // key was their contract rank, so the bridge addresses them in ascending contract rank and
        // this list is the map between the two. Not empty: `leader_index` found `me` in the list.
        let workers = u32::try_from(deployment.workers().len())
            .map_err(|_| wrong(Invalid::Unrepresentable))?;
        let mine: Box<[Rank]> = (0..workers)
            .map(Rank::from_index)
            .filter(|&contract| deployment.leader_of(contract) == Some(me))
            .collect();
        let remote_leader = deployment.workers()[mine[0].get() as usize];
        let inter = bridge(&group, &job, remote_leader, index)
            .ok_or(refused(Participant::Leader(me), "leader::open", BackendFault::Transport))?;

        Ok(Leader {
            me: Participant::Leader(me),
            inter,
            mine,
            _group: group,
            _universe: universe,
            _local: PhantomData,
        })
    }

    /// Send one frame to one worker, named by contract rank.
    ///
    /// A worker this leader does not serve is refused rather than clamped: the bridge has no rank
    /// for it.
    pub fn send(&self, to: Rank, tag: Tag, data: &[u8]) -> Result<(), Error> {
        let remote = self
            .mine
            .iter()
            .position(|&contract| contract == to)
            .ok_or(Error::Invalid(Invalid::RankOutsideJob))?;
        p2p::send_on(&self.inter, self.me, Rank::from_index(remote as u32), tag, data)
    }

    /// Take the next frame from any worker.
    ///
    /// Every frame on this route came from a worker, so there is no source to filter and none is
    /// taken. `Frame::source` is the sending worker's contract rank.
    pub fn recv(&self, out: &mut [u8]) -> Result<Option<Frame>, Error> {
        let Some((remote, tag, len)) = take(&self.inter, self.me, Owner::ALL, &mut 0, out)? else {
            return Ok(None);
        };
        // The remote rank is a position in this leader's group, not a contract rank: see `mine`.
        let source = usize::try_from(remote)
            .ok()
            .and_then(|at| self.mine.get(at).copied())
            .ok_or(Error::Invalid(Invalid::RankOutsideJob))?;
        Ok(Some(Frame::new(Some(source), tag, len)))
    }
}

/// A worker's send: one frame to this job's leader.
///
/// The destination is not a parameter and is not the caller's to choose. A bridge has a known
/// population at each end, so the leader end is one rank and this is the one place that rank is
/// written. §4 gives the worker end no destination, and this is what that costs at the point of
/// use: nothing.
pub fn send(cx: &mut Context, tag: Tag, data: &[u8]) -> Result<(), Error> {
    let me = Participant::Worker(cx.rank());
    let comm = cx.leader_route().ok_or(Error::Invalid(Invalid::NoLeader))?;
    send_on(comm, me, LEADER, tag, data)
}

/// A worker's receive: the leader's next frame. It has no source, for the reason a worker's send
/// has no destination: every frame on this route came from the leader.
pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    let me = Participant::Worker(cx.rank());
    let comm = cx.leader_route().ok_or(Error::Invalid(Invalid::NoLeader))?;
    Ok(take(comm, me, Owner::ALL, &mut 0, out)?.map(|(_, tag, len)| Frame::new(None, tag, len)))
}

impl Io<'_> {
    /// One frame to this worker's leader.
    pub fn lead(&mut self, tag: Tag, data: &[u8]) -> Result<(), Error> {
        let comm = self.leader.ok_or(Error::Invalid(Invalid::NoLeader))?;
        send_on(comm, Participant::Worker(self.rank), LEADER, tag, data)
    }
}

/// The leader's rank *in the worker group's view of the bridge*: the leader end is one rank, and
/// this is its number. Not a contract rank, and not the launch's numbering either — it is the
/// remote rank on this communicator, which is why nothing outside this module names it.
const LEADER: Rank = Rank::from_index(0);

/// The leader's published segment: it owns the mapping and the handle that names it.
pub struct Published(
    mpi_rma::Segment,
    Handle,
    // Auto traits match the nv backend's, so the public surface is the same on every backend.
    PhantomData<*const ()>,
);

/// Publish `bytes` as `revision`: write the whole object, then publish the revision.
///
/// The token is this process's id, because the segment is a node-local object its creator names.
pub fn publish(leader: &Leader, revision: NonZeroU64, bytes: &[u8]) -> Result<Published, Error> {
    let token = std::process::id();
    let length = u64::try_from(bytes.len()).map_err(|_| Error::Invalid(Invalid::Unrepresentable))?;
    let size = bytes
        .len()
        .checked_add(64)
        .ok_or(Error::Invalid(Invalid::Unrepresentable))?;
    i64::try_from(size).map_err(|_| Error::Invalid(Invalid::Unrepresentable))?;
    let segment = mpi_rma::Segment::create(&super::segment_name(token, revision), revision, bytes)
        .map_err(|e| super::os_failure(leader.me, "publish", e))?;
    Ok(Published(
        segment,
        Handle::new(revision, length, u64::from(token)),
        PhantomData,
    ))
}

/// The handle that names `segment` to its workers.
pub fn handle(segment: &Published) -> Handle {
    segment.1
}

/// Retire `segment`: unmap and unlink the publication.
pub fn retire(leader: &Leader, mut segment: Published) -> Result<(), (Published, Error)> {
    if let Err(e) = segment.0.retire() {
        return Err((segment, super::os_failure(leader.me, "retire", e)));
    }
    Ok(())
}

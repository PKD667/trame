//! The leader's route.
//!
//! §4 says what a leader is: an MPI rank with no trame worker rank, absent from worker `size` and
//! `rank`, and reaches its workers over a route of its own. §4 also leaves the
//! realization to the backend. This is this backend's, and the shape of it is the shape of the
//! statement: the leader is a **different kind of object** from a worker, not one more rank of the
//! worker population with unused fields, and what connects them is a **route of its own** rather
//! than the message route with a filter on it.
//!
//! Concretely, the job is split into one group per host's workers and one group per leader. Each
//! host's worker group builds a bridge to its one leader. Three things follow:
//!
//! - The worker group *is* the contract's participant set. A worker's rank in it is its local
//!   rank, `size` is how many there are, and the leader is not in it — so the leader is absent from
//!   `size` and `rank` by construction rather than by remembering to exclude it.
//! - The bridge's remote ranks are the workers' local ranks, because `init` keyed the split by them.
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
use std::mem::ManuallyDrop;
use std::num::NonZeroU64;

use mpi::environment::Universe;
use mpi::raw::traits::AsRaw;
use mpi::collective::CommunicatorCollectives;
use mpi::datatype::{Partition, PartitionMut};
use mpi::topology::{Color, Communicator, InterCommunicator, SimpleCommunicator};

use super::context::{Context, Environment, Io, admit, enter, leader_color, refused, return_errors};
use super::p2p::{self, send_on, take};
use crate::contract::{
    Addr, BackendFault, Deployment, Error, Failure, Frame, Handle, Invalid, Launch,
    Participant, Tag,
};
use crate::invoke::Owner;

/// The tag that labels the bridge's creation. It names the collective that builds the
/// inter-communicator and is consumed by MPI's context-id machinery, so it never reaches a message
/// and cannot collide with a caller's tag. One tag serves every host: each bridge's creation is
/// matched between a distinct pair of processes.
const BRIDGE_TAG: i32 = 42;

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
) -> Option<InterCommunicator> {
    let mut handle = unsafe { mpi::ffi::RSMPI_COMM_NULL };
    let code = unsafe {
        mpi::ffi::MPI_Intercomm_create(
            local.as_raw(),
            0,
            world.as_raw(),
            remote_leader.get() as i32,
            BRIDGE_TAG,
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
/// It has no `Context` or worker rank, and that is the point: there is nothing here for a
/// caller to mistake for a participant's state. It holds its own MPI lifetime, because a leader
/// enters MPI on its own account — `init` is the worker's entry and a leader that called it would
/// be claiming a worker rank it does not have.
///
/// Only `done` releases MPI resources: dropping this value is not a collective shutdown boundary.
pub struct Leader {
    /// Who this leader's failures are observed by: its launch rank, since it has no contract rank.
    me: Participant,
    inter: ManuallyDrop<InterCommunicator>,
    /// How many workers this leader serves: its whole deployment, the bridge's remote group,
    /// whose ranks are the workers' local ranks.
    workers: u32,
    /// The leader group, kept alive for as long as the bridge is: dropping it would disconnect a
    /// communicator the bridge was built against.
    _group: ManuallyDrop<SimpleCommunicator>,
    /// Every host's leader, ranked by host, and nobody else.
    leaders: ManuallyDrop<SimpleCommunicator>,
    _universe: ManuallyDrop<Universe>,
    // Auto traits match the nv backend's, so the public surface is the same on every backend.
    _local: PhantomData<*const ()>,
}

impl Leader {
    /// Open the route. This is the leader process's MPI entry.
    ///
    /// A leader serves its deployment's workers and no others, so it builds one bridge to their
    /// group. Both ends derive the group from the declaration.
    pub fn open(env: Environment, deployment: Deployment<'_>) -> Result<Leader, Failure> {
        let (universe, job, me, _) = enter(&env, "leader::open")?;
        let unentered = Participant::Entering(Some(me));
        let wrong = |why| refused(unentered, "leader::open", BackendFault::Invalid(why));

        // One job-wide vote with the workers: `init` and this door agree before any split or
        // bridge, so a malformed row given to another deployment refuses this leader too.
        admit(&job, deployment, false).map_err(wrong)?;

        // The link split `init` enters first. A leader is on no link: the leaders take a colour
        // of their own, keyed by host, so this split is also every host's leader in host order.
        let leaders = job
            .split_by_color_with_key(Color::with_value(1), i32::from(deployment.here()))
            .ok_or(refused(unentered, "leader::open", BackendFault::Transport))?;
        return_errors(&leaders);
        // Then the same deployment split as `init`: each host's workers take its host id, this
        // leader a colour of its own. Admission has established that this caller is the
        // deployment's named leader.
        let color = leader_color(deployment);
        let group = job
            .split_by_color_with_key(Color::with_value(color), 0)
            .ok_or(refused(unentered, "leader::open", BackendFault::Transport))?;
        let workers = u32::try_from(deployment.workers().len())
            .map_err(|_| wrong(Invalid::Unrepresentable))?;
        // Local rank 0 leads the workers' end of the bridge. Not empty: `Deployment::new` refuses
        // an empty row.
        let inter = bridge(&group, &job, deployment.workers()[0])
            .ok_or(refused(Participant::Leader(me), "leader::open", BackendFault::Transport))?;

        Ok(Leader {
            me: Participant::Leader(me),
            inter: ManuallyDrop::new(inter),
            workers,
            _group: ManuallyDrop::new(group),
            leaders: ManuallyDrop::new(leaders),
            _universe: ManuallyDrop::new(universe),
            _local: PhantomData,
        })
    }

    /// Send one frame to one worker, named by local rank.
    ///
    /// A worker outside this deployment is refused rather than clamped: the bridge has no rank
    /// for it.
    pub fn send(&self, to: u32, tag: Tag, data: &[u8]) -> Result<(), Error> {
        if to >= self.workers {
            return Err(Error::Invalid(Invalid::RankOutsideJob));
        }
        let to = i32::try_from(to).map_err(|_| Error::Invalid(Invalid::Unrepresentable))?;
        p2p::send_on(&*self.inter, self.me, to, tag, data)
    }

    /// Take the next frame from any worker.
    ///
    /// Every frame on this route came from a worker, so there is no source to filter and none is
    /// taken. `Frame::source` is the sending worker, always `Local`.
    pub fn recv(&self, out: &mut [u8]) -> Result<Option<Frame>, Error> {
        let Some((remote, tag, len)) = take(&*self.inter, self.me, Owner::ALL, &mut 0, out)? else {
            return Ok(None);
        };
        let source = u32::try_from(remote)
            .ok()
            .filter(|&at| at < self.workers)
            .ok_or(Error::Invalid(Invalid::RankOutsideJob))?;
        Ok(Some(Frame::new(Some(Addr::Local(source)), tag, len)))
    }

    /// Give `outgoing[h]` to host `h`'s leader and return what each host's leader gave this one,
    /// indexed by host, this host's own entry included. Collective over every host's leader: each
    /// enters once per exchange, in the same order, with one buffer per host.
    pub fn exchange(&self, outgoing: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, Error> {
        if outgoing.len() != self.leaders.size() as usize {
            return Err(Error::Invalid(Invalid::RankOutsideJob));
        }
        let sent = displaced(outgoing.iter().map(Vec::len))?;
        let mut counts = vec![0i32; outgoing.len()];
        self.leaders.all_to_all_into(&sent.0[..], &mut counts[..]);
        let got = displaced(counts.iter().map(|&n| n as usize))?;
        let flat = outgoing.concat();
        let mut into = vec![0u8; got.2];
        {
            let send = Partition::new(&flat[..], &sent.0[..], &sent.1[..]);
            let mut recv = PartitionMut::new(&mut into[..], &got.0[..], &got.1[..]);
            self.leaders.all_to_all_varcount_into(&send, &mut recv);
        }
        Ok(got.0.iter().zip(&got.1).map(|(&n, &at)| into[at as usize..][..n as usize].to_vec()).collect())
    }

    /// Finalize explicitly with the workers, discarding unread frames rather than delivering them.
    pub fn done<A>(&mut self, outcome: Result<(), Failure<A>>) -> Result<(), Failure<A>> {
        super::context::quiesce(&mut self._universe, |out| {
            take(&*self.inter, self.me, Owner::ALL, &mut 0, out)?;
            Ok(())
        });
        // SAFETY: the shutdown agreement completed and no application call is outstanding.
        unsafe {
            ManuallyDrop::drop(&mut self.inter);
            ManuallyDrop::drop(&mut self._group);
            ManuallyDrop::drop(&mut self.leaders);
            ManuallyDrop::drop(&mut self._universe);
        }
        outcome
    }
}

/// MPI counts and displacements of consecutive buffers of these lengths, and their total, all of
/// which MPI carries as `i32`.
fn displaced(lengths: impl Iterator<Item = usize>) -> Result<(Vec<i32>, Vec<i32>, usize), Error> {
    let wide = |n: usize| i32::try_from(n).map_err(|_| Error::Invalid(Invalid::Unrepresentable));
    let (mut counts, mut at, mut total) = (Vec::new(), Vec::new(), 0usize);
    for n in lengths {
        counts.push(wide(n)?);
        at.push(wide(total)?);
        total += n;
    }
    wide(total)?;
    Ok((counts, at, total))
}

/// A worker's send: one frame to this deployment's leader.
///
/// The destination is not a parameter and is not the caller's to choose. A bridge has a known
/// population at each end, so the leader end is one rank and this is the one place that rank is
/// written. §4 gives the worker end no destination, and this is what that costs at the point of
/// use: nothing.
pub fn send(cx: &mut Context, tag: Tag, data: &[u8]) -> Result<(), Error> {
    let me = Participant::Worker(cx.rank());
    let comm = cx.leader_route();
    send_on(comm, me, LEADER, tag, data)
}

/// A worker's receive: the leader's next frame. It has no source, for the reason a worker's send
/// has no destination: every frame on this route came from the leader.
pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error> {
    let me = Participant::Worker(cx.rank());
    let comm = cx.leader_route();
    Ok(take(comm, me, Owner::ALL, &mut 0, out)?.map(|(_, tag, len)| Frame::new(None, tag, len)))
}

impl Io<'_> {
    /// One frame to this worker's leader.
    pub fn lead(&mut self, tag: Tag, data: &[u8]) -> Result<(), Error> {
        let comm = self.leader;
        send_on(comm, Participant::Worker(self.rank), LEADER, tag, data)
    }
}

/// The leader's rank *in the worker group's view of the bridge*: the leader end is one rank, and
/// this is its number. Not a contract rank, and not the launch's numbering either — it is the
/// remote rank on this communicator, which is why nothing outside this module names it.
const LEADER: i32 = 0;

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
    let segment = mpi_rma::Segment::create(&super::segment::segment_name(token, revision), revision, bytes)
        .map_err(|e| super::segment::os_failure(leader.me, "publish", e))?;
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
        return Err((segment, super::segment::os_failure(leader.me, "retire", e)));
    }
    Ok(())
}

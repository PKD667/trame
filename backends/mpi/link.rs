// The launch's link: how a worker reaches a worker of another host.
//
// One communicator for every worker of the launch table, whatever its host, split from the job
// with colour 0 and the job rank as key. So a worker's rank on the link is the number of table
// workers whose launch rank is below its own, and every process computes the whole map from the
// table it was given, with no collective. A leader is on no link: leaders never talk.
//
// A worker of this host is always reached on this deployment's communicator, and a worker of
// another host always on the link. A source uses exactly one communicator per receiver, so MPI's
// order per communicator is the contract's FIFO per directed pair. A link frame is a buffered send
// like any other, so it is never dropped, even on the backend whose lanes may skip.

use mpi::topology::{Communicator, SimpleCommunicator};

use super::context::{failure, return_errors};
use super::p2p::{self, send_on};
use crate::contract::{Addr, Deployment, Edge, Error, Frame, Invalid, Launch, Participant, Tag};
use crate::invoke::Owner;

/// The link and the two maps between its ranks and the launch's addresses.
pub(crate) struct Link {
    comm: SimpleCommunicator,
    /// `of[h][r]`: the link rank of host `h`'s worker `r`. Row `here` gives only its length,
    /// because a worker of this host is reached on the deployment's communicator.
    of: Box<[Box<[i32]>]>,
    /// By link rank: the address that worker has from here. `Local` on this host and `Remote`
    /// elsewhere, so a source is canonical as it is read.
    from: Box<[Addr]>,
    here: u16,
}

/// Where an address is: this deployment's communicator, or the link, with the rank on it.
pub(crate) enum Peer {
    Here(i32),
    There(i32),
}

/// This worker's lanes to other hosts in the current load: the workers its edges send to, and the
/// declared frame length. A window never reaches another host, so these are checked here.
#[derive(Default)]
pub(crate) struct Far {
    to: Vec<Addr>,
    frame: usize,
}

impl Link {
    /// Take the link communicator `init` split and build both maps from the table.
    ///
    /// `InconsistentLaunch` when the link does not hold exactly the table's workers: some process
    /// was given another table, or entered through the other door.
    pub(super) fn new(comm: SimpleCommunicator, deployment: Deployment<'_>) -> Result<Link, Invalid> {
        let table = deployment.hosts();
        let here = deployment.here();
        let workers: usize = table.iter().map(|row| row.len()).sum();
        if usize::try_from(comm.size()) != Ok(workers) {
            return Err(Invalid::InconsistentLaunch);
        }
        return_errors(&comm);
        // `Deployment::new` refused more than 65,536 hosts and a row longer than `u32::MAX`, so
        // both narrowings are exact.
        let mut by_launch: Vec<(Launch, Addr)> = table
            .iter()
            .enumerate()
            .flat_map(|(h, row)| {
                row.iter().enumerate().map(move |(r, &launch)| {
                    let addr = if h == usize::from(here) {
                        Addr::Local(r as u32)
                    } else {
                        Addr::Remote { host: h as u16, rank: r as u32 }
                    };
                    (launch, addr)
                })
            })
            .collect();
        by_launch.sort_unstable_by_key(|&(launch, _)| launch);
        // The position in launch order is the link rank. Below `comm.size()`, so it fits.
        let at = |launch: &Launch| {
            by_launch
                .binary_search_by_key(launch, |&(l, _)| l)
                .expect("every table worker is in the table") as i32
        };
        let of = table.iter().map(|row| row.iter().map(at).collect()).collect();
        let from = by_launch.iter().map(|&(_, addr)| addr).collect();
        Ok(Link { comm, of, from, here })
    }
}

/// Where `to` is: the one place an address becomes an MPI rank. An address that names no worker
/// of the launch — a `Local` past this deployment, a `Remote` naming this host, a host outside
/// the launch, a rank past its host's row — is `RankOutsideJob`.
pub(crate) fn route(link: &Link, to: Addr) -> Result<Peer, Error> {
    let peer = match to {
        // A local rank is below this row's length, which is below the link's size: it fits.
        Addr::Local(rank) => link.of[usize::from(link.here)]
            .get(rank as usize)
            .map(|_| Peer::Here(rank as i32)),
        Addr::Remote { host, rank } if host != link.here => link
            .of
            .get(usize::from(host))
            .and_then(|row| row.get(rank as usize))
            .map(|&at| Peer::There(at)),
        Addr::Remote { .. } => None,
    };
    peer.ok_or(Error::Invalid(Invalid::RankOutsideJob))
}

/// A frame to `to` on whichever communicator reaches it.
pub(crate) fn send(
    world: &SimpleCommunicator,
    link: &Link,
    me: u32,
    to: Addr,
    tag: Tag,
    data: &[u8],
) -> Result<(), Error> {
    let me = Participant::Worker(me);
    match route(link, to)? {
        Peer::Here(at) => send_on(world, me, at, tag, data),
        Peer::There(at) => send_on(&link.comm, me, at, tag, data),
    }
}

/// A lane frame to `to`, a worker of another host at `at` on the link, under the load's `tag`.
/// The pair must be one this worker's edges declared, and the frame must fit the declared length.
pub(crate) fn lane(
    link: &Link,
    far: &Far,
    me: u32,
    to: Addr,
    at: i32,
    tag: Tag,
    data: &[u8],
) -> Result<(), Error> {
    if !far.to.contains(&to) {
        return Err(Error::Invalid(Invalid::NoLane));
    }
    if data.len() > far.frame {
        return Err(Error::TooLarge { limit: far.frame });
    }
    send_on(&link.comm, Participant::Worker(me), at, tag, data)
}

/// The lanes a load declares to other hosts, once every end of every edge names a worker of the
/// launch. `cpu::lanes::validate` has already checked the `Local` ends; this checks the `Remote`
/// ones, which only the table can.
pub(crate) fn far(link: &Link, me: u32, edges: &[Edge], frame: usize) -> Result<Far, Error> {
    let mut to = Vec::new();
    for edge in edges {
        route(link, edge.source())?;
        route(link, edge.destination())?;
        if edge.source() == Addr::Local(me) && matches!(edge.destination(), Addr::Remote { .. }) {
            to.push(edge.destination());
        }
    }
    Ok(Far { to, frame })
}

/// One attempt at the next frame `owner` receives on the link, named by its sender's address.
pub(crate) fn take(
    link: &Link,
    me: u32,
    owner: Owner<'_>,
    turn: &mut usize,
    out: &mut [u8],
) -> Result<Option<Frame>, Error> {
    let Some((source, tag, len)) = p2p::take(&link.comm, Participant::Worker(me), owner, turn, out)?
    else {
        return Ok(None);
    };
    let from = usize::try_from(source)
        .ok()
        .and_then(|at| link.from.get(at).copied())
        .ok_or(failure(me, "recv"))?;
    Ok(Some(Frame::new(Some(from), tag, len)))
}

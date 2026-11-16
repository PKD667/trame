// The hosts of an MPI world (CONTRACT.md, Federation). A host is a shared-memory node: the ranks of
// `MPI_Comm_split_type(SHARED)`, cut into groups of TRAME_MPI_NODE_RANKS consecutive ranks for tests on
// one machine (nodes.rs). Hosts are numbered 0..count in order of their lowest world rank (F1). Each
// host launches on its own communicator, as a one-node backend: its own window, segment, leader (its
// last rank) and workers. A route between workers of two hosts goes rank to rank on `cross`, a
// duplicate of the world (transport/cross/). The leaders of the hosts form a communicator of their own, the
// peer channel (F6), ranked by host.
//
// The table is made at the first call, collectively over the world: every rank of the world calls
// `host` before its launch (backend.rs `agree`), so every rank reaches the splits.

extern crate std;

use std::vec::Vec;

use mpi::point_to_point::{Destination, Source};
use mpi::topology::{Color, Communicator, SimpleCommunicator};
use mpi::traits::*;

use crate::backends::mpi::setup::state::{local, world};
use crate::backends::mpi::transport::nodes::{discover, node_ranks_setting, split_groups};

pub(crate) struct Host {
    /// This host's communicator: its launch's world.
    pub comm: SimpleCommunicator,
    /// This host's number, and the number of hosts.
    pub id: u32,
    pub count: u32,
    /// The leaders' communicator, rank = host, on the leader of each host only.
    pub peers: Option<SimpleCommunicator>,
    /// Peer messages received but not yet taken: (host, tag, bytes).
    pub pending: Vec<(u32, u64, Vec<u8>)>,
    /// Peer messages received and sent so far: for the leader's `wait` (leader.rs), and as messages
    /// in flight for the detector (detect.rs).
    pub arrivals: u64,
    pub sent: u64,
    /// The leader's deadlock detector, from its launch on (detect.rs).
    pub watch: Option<crate::backends::mpi::roles::detect::Watch>,
    /// Per host, its world ranks: worker k of host h is `members[h][k]`, its leader the last.
    pub members: Vec<Vec<i32>>,
    /// The world, duplicated for the frames and credits of routes between hosts (transport/cross/).
    pub cross: SimpleCommunicator,
}

/// One peer message tag for every peer message: the F4 tag travels in the first 8 bytes.
const PEER_TAG: i32 = 7;

/// This process's host, made at the first call (collective over the world, see the module comment).
pub(crate) fn host() -> &'static mut Host {
    if local().host.is_none() {
        let w = world();
        let setting = node_ranks_setting();
        let shared = w.split_shared(w.rank());
        let comm = match setting {
            Ok(Some(k)) => {
                let g = split_groups(w, &shared, k).expect("a rank's own colour is never undefined");
                std::mem::forget(shared);
                g
            }
            _ => shared,
        };
        let nodes = discover(w, &comm);
        let leader = comm.rank() == comm.size() - 1;
        let colour = if leader { Color::with_value(0) } else { Color::undefined() };
        let peers = w.split_by_color_with_key(colour, nodes.me as i32);
        let cross = w.duplicate();
        local().host = Some(Host {
            comm,
            id: nodes.me,
            count: nodes.count,
            peers,
            pending: Vec::new(),
            arrivals: 0,
            sent: 0,
            watch: None,
            members: nodes.members,
            cross,
        });
    }
    local().host.as_mut().expect("the host was just set")
}

/// Send a peer message (F6): blocking, so `Full` is never answered; MPI buffers small messages.
pub(crate) fn send(to: u32, tag: u64, bytes: &[u8]) {
    let h = host();
    assert!(to != h.id && to < h.count, "trame: host {to} is not a peer of host {}", h.id);
    let peers = h.peers.as_ref().expect("trame: only a leader has peers");
    let mut msg = Vec::with_capacity(8 + bytes.len());
    msg.extend_from_slice(&tag.to_le_bytes());
    msg.extend_from_slice(bytes);
    peers.process_at_rank(to as i32).send_with_tag(&msg[..], PEER_TAG);
    h.sent += 1;
}

/// Move every peer message MPI holds for this leader into `pending`. Answers whether one came.
pub(crate) fn drain() -> bool {
    let h = host();
    let Some(peers) = h.peers.as_ref() else { return false };
    let mut any = false;
    while let Some((msg, status)) = peers.any_process().immediate_matched_probe_with_tag(PEER_TAG) {
        let n = status.count(u8::equivalent_datatype()) as usize;
        let mut bytes = std::vec![0u8; n];
        msg.matched_receive_into(&mut bytes[..]);
        let tag = u64::from_le_bytes(bytes[..8].try_into().expect("a peer message starts with its tag"));
        h.pending.push((status.source_rank() as u32, tag, bytes[8..].to_vec()));
        h.arrivals += 1;
        any = true;
    }
    any
}

/// Take the oldest pending peer message of tag `tag` into `out`; answers its host.
pub(crate) fn recv(tag: u64, out: &mut [u8]) -> Option<u32> {
    drain();
    let h = host();
    let i = h.pending.iter().position(|(_, t, _)| *t == tag)?;
    let (from, _, bytes) = h.pending.remove(i);
    assert_eq!(bytes.len(), out.len(), "trame: a peer message of this tag has another size");
    out.copy_from_slice(&bytes);
    Some(from)
}

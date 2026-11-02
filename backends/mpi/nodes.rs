// The nodes of a launch: which shared-memory node each world rank is on. A node is a set of ranks that
// share memory (`MPI_Comm_split_type(SHARED)`), optionally cut into groups of TRAME_MPI_NODE_RANKS
// consecutive ranks for tests on one machine (NODES.md, Testing on one machine). Every rank runs
// `discover`, which is collective over the world communicator, so every rank gets the same table.

extern crate std;

use std::env;
use std::string::String;
use std::vec::Vec;

use mpi::collective::SystemOperation;
use mpi::topology::{Color, SimpleCommunicator};
use mpi::traits::*;

/// The node table, by world rank. Node numbers are 0..count, in order of each node's lowest world rank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Nodes {
    /// The node of each world rank: len W + 1, the leader's rank last.
    pub node_of: Vec<u32>,
    /// This rank's node.
    pub me: u32,
    /// The number of nodes.
    pub count: u32,
    /// Per node, the lowest world rank on it: the rank that hosts the node's relay.
    pub relay_rank: Vec<i32>,
    /// The agent ranks on this rank's node, in rank order: the workers, then the leader's rank W if the
    /// leader is here. The relay's per-wake scans run over this.
    pub local: Vec<u32>,
}

/// The value of TRAME_MPI_NODE_RANKS: `Ok(None)` when unset, `Ok(Some(k))` for k >= 1. Each rank reads
/// its own environment, and `open` checks that all ranks agree (a refusal, on every rank).
pub(crate) fn node_ranks_setting() -> Result<Option<i32>, String> {
    let Ok(text) = env::var("TRAME_MPI_NODE_RANKS") else {
        return Ok(None);
    };
    match text.parse::<i32>() {
        Ok(k) if k >= 1 => Ok(Some(k)),
        _ => Err(text),
    }
}

/// Cut the shared-memory communicator `shared` into groups of `k` consecutive world ranks. Collective
/// over `shared`. The result is the node communicator for the window and the relay.
pub(crate) fn split_groups(
    world: &SimpleCommunicator,
    shared: &SimpleCommunicator,
    k: i32,
) -> Option<SimpleCommunicator> {
    shared.split_by_color(Color::with_value(world.rank() / k))
}

/// The node table. Collective over `world`: `node` is this rank's node communicator (split_groups or
/// the shared one), and every rank of `world` calls this. A node is numbered by its lowest world rank.
pub(crate) fn discover(world: &SimpleCommunicator, node: &SimpleCommunicator) -> Nodes {
    let rank = world.rank();
    let mut low = 0i32;
    node.all_reduce_into(&rank, &mut low, SystemOperation::min());
    let mut lows: Vec<i32> = std::vec![0i32; world.size() as usize];
    world.all_gather_into(&low, &mut lows[..]);
    let mut relay_rank = lows.clone();
    relay_rank.sort_unstable();
    relay_rank.dedup();
    let node_of: Vec<u32> =
        lows.iter().map(|l| relay_rank.binary_search(l).expect("every lowest rank is in the table") as u32).collect();
    let me = node_of[rank as usize];
    let local: Vec<u32> = (0..node_of.len() as u32).filter(|&w| node_of[w as usize] == me).collect();
    Nodes { node_of, me, count: relay_rank.len() as u32, relay_rank, local }
}

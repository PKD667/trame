// The nodes of a launch: which shared-memory node each world rank is on. A node is a set of ranks that
// share memory (`MPI_Comm_split_type(SHARED)`), optionally cut into groups of TRAME_MPI_NODE_RANKS
// consecutive ranks for tests on one machine (transport/README.md, Testing on one machine). Every rank runs
// `discover`, which is collective over the world communicator, so every rank gets the same table.

extern crate std;

use std::env;
use std::string::String;
use std::vec::Vec;

use mpi::collective::SystemOperation;
use mpi::topology::{Color, SimpleCommunicator};
use mpi::traits::*;

/// The node table. Node numbers are 0..count, in order of each node's lowest world rank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Nodes {
    /// This rank's node.
    pub me: u32,
    /// The number of nodes.
    pub count: u32,
    /// Per node, its world ranks in order: rank k of the node's communicator is `members[node][k]`.
    pub members: Vec<Vec<i32>>,
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
/// over `shared`. The result is the node communicator, for the window and the segment.
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
    let mut firsts = lows.clone();
    firsts.sort_unstable();
    firsts.dedup();
    let node_of = |l: &i32| firsts.binary_search(l).expect("every lowest rank is in the table");
    // The node communicator is split with the world rank as key, so its ranks are in world order.
    let mut members = std::vec![Vec::new(); firsts.len()];
    for (r, l) in lows.iter().enumerate() {
        members[node_of(l)].push(r as i32);
    }
    Nodes { me: node_of(&low) as u32, count: firsts.len() as u32, members }
}

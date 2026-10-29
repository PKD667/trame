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
    /// The workers (ranks below W) on this rank's node.
    pub local_workers: u32,
    /// The agent ranks on this rank's node, in rank order: the workers, then the leader's rank W if the
    /// leader is here. The per-wake and per-barrier scans of the relay and of `bump_local` run over this.
    pub local: Vec<u32>,
    /// The nodes with at least one worker: the nodes whose arrivals the root waits for. A node with only
    /// the leader has no barrier arrivals, so it never sends ARRIVED.
    pub arrivers: u32,
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

/// The barrier generation that a node's COUNT value `count` completes, if it completes one. `local` is
/// the node's workers and `workers` is W. A node adds one per local arrival and, after each release, the
/// other nodes' share, so arrivals of generation g leave COUNT at (g - 1) W + j for j = 1..=local. The
/// test is `count - local` a multiple of W: it holds exactly at j = local. `local` = W is covered, and
/// `local` = 0 never completes (that node has no arrivals).
pub(crate) fn arrival_done(count: u64, workers: u64, local: u64) -> Option<u64> {
    if local == 0 || count < local || !(count - local).is_multiple_of(workers) {
        return None;
    }
    Some((count - local) / workers + 1)
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
    let workers = world.size() as usize - 1;
    let local: Vec<u32> = (0..node_of.len() as u32).filter(|&w| node_of[w as usize] == me).collect();
    let local_workers = local.iter().filter(|&&w| (w as usize) < workers).count() as u32;
    let arrivers = (0..relay_rank.len() as u32).filter(|n| node_of[..workers].contains(n)).count() as u32;
    Nodes { node_of, me, count: relay_rank.len() as u32, relay_rank, local_workers, local, arrivers }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::arrival_done;

    #[test]
    fn arrivals_complete_once_per_generation() {
        // Five workers, two of them on this node, COUNT as the gate adds to it over two generations.
        let done: Vec<Option<u64>> = (0..=12).map(|c| arrival_done(c, 5, 2)).collect();
        assert_eq!(done[2], Some(1));
        assert_eq!(done[7], Some(2));
        assert_eq!(done[12], Some(3));
        assert_eq!(done.iter().flatten().count(), 3, "{done:?}");
        // All workers on this node: the last arrival of each generation is at a multiple of W.
        assert_eq!(arrival_done(3, 3, 3), Some(1));
        assert_eq!(arrival_done(6, 3, 3), Some(2));
        assert_eq!(arrival_done(0, 3, 3), None);
        // No worker on this node: it never completes a generation.
        assert_eq!(arrival_done(5, 5, 0), None);
    }
}

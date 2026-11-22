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

/// The value of TRAME_MPI_NODE_RANKS: how a node's consecutive world ranks are cut into hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NodeRanks {
    /// `k`: every host has k ranks.
    Each(i32),
    /// `k0,k1,...`: host 0 has k0 ranks, host 1 has k1, and so on; the sizes add up to the world's.
    Sizes(Vec<i32>),
}

impl NodeRanks {
    /// The host of world rank `rank`.
    pub(crate) fn host_of(&self, rank: i32) -> i32 {
        match self {
            NodeRanks::Each(k) => rank / k,
            NodeRanks::Sizes(sizes) => {
                let mut end = 0;
                for (h, k) in sizes.iter().enumerate() {
                    end += k;
                    if rank < end {
                        return h as i32;
                    }
                }
                sizes.len() as i32 - 1
            }
        }
    }

    /// The number of ranks the setting needs, when it fixes one.
    pub(crate) fn ranks(&self) -> Option<i32> {
        match self {
            NodeRanks::Each(_) => None,
            NodeRanks::Sizes(sizes) => Some(sizes.iter().sum()),
        }
    }
}

/// The value of `text`: one number of 1 or more, or several separated by commas.
pub(crate) fn parse_node_ranks(text: &str) -> Option<NodeRanks> {
    let mut sizes = Vec::new();
    for part in text.split(',') {
        match part.trim().parse::<i32>() {
            Ok(k) if k >= 1 => sizes.push(k),
            _ => return None,
        }
    }
    if sizes.len() == 1 { Some(NodeRanks::Each(sizes[0])) } else { Some(NodeRanks::Sizes(sizes)) }
}

/// The value of TRAME_MPI_NODE_RANKS: `Ok(None)` when unset. Each rank reads its own environment, and
/// `open` checks that all ranks agree (a refusal, on every rank).
pub(crate) fn node_ranks_setting() -> Result<Option<NodeRanks>, String> {
    let Ok(text) = env::var("TRAME_MPI_NODE_RANKS") else {
        return Ok(None);
    };
    parse_node_ranks(&text).map(Some).ok_or(text)
}

/// Cut the shared-memory communicator `shared` into hosts by `setting`. Collective over `shared`. The
/// result is the node communicator, for the window and the segment.
pub(crate) fn split_groups(
    world: &SimpleCommunicator,
    shared: &SimpleCommunicator,
    setting: &NodeRanks,
) -> Option<SimpleCommunicator> {
    shared.split_by_color(Color::with_value(setting.host_of(world.rank())))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_size_or_a_list() {
        assert_eq!(parse_node_ranks("3"), Some(NodeRanks::Each(3)));
        assert_eq!(parse_node_ranks("3,1,2"), Some(NodeRanks::Sizes(std::vec![3, 1, 2])));
        for bad in ["", "0", "1,0", "a", "1,,2", "-2"] {
            assert_eq!(parse_node_ranks(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_list_cuts_consecutive_ranks() {
        let s = parse_node_ranks("3,1,2").unwrap();
        let hosts: Vec<i32> = (0..6).map(|r| s.host_of(r)).collect();
        assert_eq!(hosts, std::vec![0, 0, 0, 1, 2, 2]);
        assert_eq!(s.ranks(), Some(6));
        assert_eq!(NodeRanks::Each(2).host_of(5), 2);
    }
}

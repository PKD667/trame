// Which path a small frame takes between nodes (RMA.md). The setting is TRAME_MPI_SMALL: unset or `p2p`
// sends every frame by MPI_Send (the default), `rma` sends small frames over one lane per ordered node pair
// (relay_rma.rs). The predicate and the lane plan are pure, so every rank computes the same answer from the
// same topology and node table.

extern crate std;

use std::collections::BTreeMap;
use std::env;
use std::string::String;
use std::vec::Vec;

use super::layout::Route;

/// The largest route size, in bytes, that takes the RMA path. A constant, not a setting: a route's path is
/// chosen by its size alone, and a measured choice belongs in the code, not in a launch variable.
pub const SMALL_BYTES: usize = 4096;

/// What a lane frame carries before its payload: the route (u32) and the frame's index (u64).
pub(crate) const LANE_HEAD: usize = 12;

/// The small-frame path of a launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Path {
    P2p,
    Rma,
}

/// The value of TRAME_MPI_SMALL. `Err` carries the value that is not `p2p`, `rma`, or unset. Each rank
/// reads its own environment, and `open` checks that all ranks agree.
pub(crate) fn small_setting() -> Result<Path, String> {
    let Some(text) = env::var_os("TRAME_MPI_SMALL") else {
        return Ok(Path::P2p);
    };
    match text.to_str() {
        Some("p2p") => Ok(Path::P2p),
        Some("rma") => Ok(Path::Rma),
        _ => Err(text.to_string_lossy().into_owned()),
    }
}

/// Whether a route takes the RMA path: the launch is rma, the route is lossless, its ends are on different
/// nodes, and its frames are small. Lossy routes stay on p2p, because their overwrite semantics are not the
/// lane's.
pub(crate) fn is_small(rma: bool, from: u32, to: u32, size: usize, lossy: bool, node_of: &[u32]) -> bool {
    rma && !lossy && node_of[from as usize] != node_of[to as usize] && size <= SMALL_BYTES
}

/// The lanes of a launch, as `Ring::raw` takes them: `(source relay, destination relay, depth, capacity)`,
/// one per ordered node pair that has a small route. Depth is the sum of the capacities of the routes on the
/// lane: a route holds at most `cap` frames unacknowledged (spec 5.2, Full), so the lane never holds an unread
/// frame that it must overwrite (RMA.md, Why the lane cannot overwrite). Capacity is the largest frame on it.
pub(crate) fn lanes(routes: &[Route], node_of: &[u32], relay_rank: &[i32], rma: bool) -> Vec<(i32, i32, usize, usize)> {
    let mut by: BTreeMap<(u32, u32), (usize, usize)> = BTreeMap::new();
    for r in routes {
        if !is_small(rma, r.from, r.to, r.size, r.lossy, node_of) {
            continue;
        }
        let lane = by.entry((node_of[r.from as usize], node_of[r.to as usize])).or_insert((0, 0));
        lane.0 += r.cap as usize;
        lane.1 = lane.1.max(r.size + LANE_HEAD);
    }
    by.into_iter().map(|((a, b), (depth, capacity))| (relay_rank[a as usize], relay_rank[b as usize], depth, capacity)).collect()
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn small_routes_are_lossless_cross_node_and_under_the_bound() {
        // Ranks 0,1 on node 0; rank 2 on node 1.
        let node_of = [0u32, 0, 1];
        assert!(is_small(true, 0, 2, 16, false, &node_of));
        assert!(is_small(true, 0, 2, SMALL_BYTES, false, &node_of));
        assert!(!is_small(true, 0, 2, SMALL_BYTES + 1, false, &node_of), "over the bound");
        assert!(!is_small(true, 0, 1, 16, false, &node_of), "same node");
        assert!(!is_small(true, 0, 2, 16, true, &node_of), "lossy");
        assert!(!is_small(false, 0, 2, 16, false, &node_of), "p2p launch");
    }

    #[test]
    fn one_lane_per_ordered_node_pair_with_depth_the_sum_of_capacities() {
        let node_of = [0u32, 0, 1, 1];
        let relay_rank = [0i32, 2];
        let route = |from, to, size, cap| Route { from, to, tag: 0, size, cap, lossy: false };
        let routes = [
            route(0, 2, 16, 3),
            route(1, 3, 200, 4),  // same node pair: joins the lane
            route(2, 0, 8, 2),    // the other direction: its own lane
            route(0, 1, 16, 9),   // same node: not a lane
            route(0, 2, 8192, 5), // over the bound: not a lane
        ];
        assert_eq!(
            lanes(&routes, &node_of, &relay_rank, true),
            vec![(0, 2, 7, 200 + LANE_HEAD), (2, 0, 2, 8 + LANE_HEAD)]
        );
        assert!(lanes(&routes, &node_of, &relay_rank, false).is_empty());
    }
}

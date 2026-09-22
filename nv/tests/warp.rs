//! The device module's tests: the partition `#[parallel]` is lowered to, the lane identity,
//! and the launch fabric.
//!
//! These are tests of the machinery `trame/nv/` is built on. What a
//! *declaration* promises is tested in `declare.rs`, where the attributes are exercised.

use crate::nv::warp::{self, LANES, Split};

#[test]
fn the_lanes_partition_a_range_exactly() {
    // The property the whole lowering rests on: the parts are disjoint and their union is the
    // range, for every length and every offset — including the lengths either side of a warp
    // and the empty range.
    for lo in [0u32, 1, 31] {
        for len in [0u32, 1, 2, 31, 32, 33, 64, 65, 100] {
            let hi = lo + len;
            let mut seen: Vec<u32> = Vec::new();
            warp::sim(|| {
                let mut range = Split::new(lo, hi);
                while range.more() {
                    seen.push(range.at());
                    range.step();
                }
            });
            seen.sort_unstable();
            let want: Vec<u32> = (lo..hi).collect();
            assert_eq!(seen, want, "range {lo}..{hi} was not partitioned exactly");
            seen.clear();
        }
    }
}

#[test]
fn one_lane_of_a_short_range_carries_all_of_it() {
    // Three indices, 32 lanes: lanes 0, 1 and 2 get one each and the rest get nothing. This is
    // the shape that distinguishes a partition from a replication, and it is what
    // `declare.rs` uses to tell them apart through `invoke!`.
    let owner = warp::sim(|| {
        let mut range = Split::new(0u32, 3);
        let mut mine = 0;
        while range.more() {
            mine += 1;
            range.step();
        }
        (warp::lane(), mine)
    });
    assert_eq!(
        owner,
        vec![(0, 1), (1, 1), (2, 1)]
            .into_iter()
            .chain((3..LANES).map(|k| (k, 0)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_stride_is_one_warp() {
    // The second index a lane visits is one warp along, not one index along: a stride of one
    // would visit every index on every lane.
    let visited: Vec<u32> = warp::sim_lane(1, || {
        let mut range = Split::new(0u32, 100);
        let mut mine = Vec::new();
        while range.more() {
            mine.push(range.at());
            range.step();
        }
        mine
    });
    assert_eq!(visited, vec![1, 33, 65, 97]);
}

#[test]
fn a_lane_outside_a_warp_is_zero_and_a_context_is_restored() {
    // A host caller with no warp around it is lane 0, so a `#[parallel]` invoked outside a warp
    // runs the lane-0 share and nothing else. `sim_warp` is a context, not a thread, and it
    // puts the previous one back.
    assert_eq!(warp::lane(), 0);
    assert_eq!(warp::here_id(), 0);
    assert_eq!(
        warp::sim_warp(3, || (warp::here_id(), warp::lane())),
        (3, 0)
    );
    assert_eq!(warp::here_id(), 0);
}

// The launch record used to be tested here, as a global. It is not one any more: identity and
// endpoints are participant-local state the caller owns, so per-participant identity is checked
// where the backend that owns the context is, in `trame/tests/nv.rs`. A test of a global that
// no longer exists would be a test of nothing.

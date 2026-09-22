// The RMA window's lane table, which is the one piece of a ring backend that needs no
// communicator and therefore the piece `tests/` can check.
//
// `plan` used to live in `rma/`. What replaced it is the shared `window` rule, because the three
// ring transports open the same table and only differ in what they build from it.

use std::collections::HashMap;

use crate::contract::Edge;
use crate::shared::context::FACTOR;

/// The declaration the tests work from: edges ordered by `(source, destination)`, workers ascending
/// and unique, which is what the geometry rules require and therefore what a test may hand over.
fn edges(pairs: &[(u32, u32)]) -> Vec<Edge> {
    pairs
        .iter()
        .map(|&(source, destination)| Edge {
            source,
            destination,
            affected: 1,
        })
        .collect()
}

#[test]
fn the_window_is_indexed_by_position_in_the_worker_list() {
    let workers = [3u32, 1, 2];
    let got = crate::shared::context::window(&workers, &edges(&[(3, 2)]), 64).expect("a table");
    assert_eq!(got.len(), 1);
    // Rank 3 is first in the worker list and rank 2 is last, and a window is indexed by place
    // rather than by rank, so the two are not the same number.
    assert_eq!(got, vec![(0, 2, FACTOR, 64)]);
}

#[test]
fn the_window_is_sorted_so_every_member_opens_the_same_one() {
    let workers = [0u32, 1, 2];
    let got = crate::shared::context::window(&workers, &edges(&[(2, 0), (0, 1), (1, 2)]), 8)
        .expect("a table");
    let mut sorted = got.clone();
    sorted.sort_unstable();
    assert_eq!(
        got, sorted,
        "a window built from an unsorted table is a different window"
    );
}

#[test]
fn a_table_with_nothing_on_it_is_empty() {
    let got = crate::shared::context::window(&[0, 1], &[], 64).expect("a table");
    assert!(got.is_empty());
}

#[test]
fn an_edge_whose_endpoint_is_not_a_worker_is_refused() {
    // Not a panic and not a silent skip: the endpoint has no place in the worker list, so the
    // table cannot name it and the declaration is refused.
    let _ = HashMap::<(), ()>::new();
    assert!(crate::shared::context::window(&[0, 1], &edges(&[(0, 9)]), 64).is_err());
}

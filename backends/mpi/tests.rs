// The RMA window's lane table, which is the one piece of a ring backend that needs no
// communicator and therefore the piece `tests/` can check.
//
// `plan` used to live in `rma/`. What replaced it is the shared `window` rule, because the three
// ring transports open the same table and only differ in what they build from it.

use std::collections::HashMap;

use std::num::NonZeroU32;

use crate::contract::{Addr, Edge, Participant, Tag};
use crate::invoke::{Owner, Receive};
use crate::mpi::context::FACTOR;

/// The declaration the tests work from: edges ordered by `(source, destination)`, workers ascending
/// and unique, which is what the geometry rules require and therefore what a test may hand over.
fn edges(pairs: &[(u32, u32)]) -> Vec<Edge> {
    pairs
        .iter()
        .map(|&(source, destination)| {
            Edge::new(Addr::Local(source), Addr::Local(destination), NonZeroU32::MIN)
        })
        .collect()
}

#[test]
fn the_window_is_indexed_by_position_in_the_worker_list() {
    let workers = [3, 1, 2];
    let got = crate::mpi::context::window(&workers, &edges(&[(3, 2)]), 64)
        .expect("a table");
    assert_eq!(got.len(), 1);
    // Rank 3 is first in the worker list and rank 2 is last, and a window is indexed by place
    // rather than by rank, so the two are not the same number.
    assert_eq!(got, vec![(0, 2, FACTOR, 64)]);
}

#[test]
fn the_window_is_sorted_so_every_member_opens_the_same_one() {
    let workers = [0, 1, 2];
    let table = edges(&[(2, 0), (0, 1), (1, 2)]);
    let got = crate::mpi::context::window(&workers, &table, 8)
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
    let got = crate::mpi::context::window(&[0, 1], &[], 64).expect("a table");
    assert!(got.is_empty());
}

#[test]
fn scoped_all_progresses_past_a_queued_unowned_frame_and_preserves_it() {
    let deferred = crate::mpi::p2p::Deferred::new();
    let unowned = Tag::new(11);
    let owned = Tag::new(12);
    let me = Participant::Worker(0);
    deferred.push((3, unowned, vec![1, 2]), me).expect("defer unowned");
    deferred.push((3, unowned, vec![3]), me).expect("defer next unowned");
    deferred.push((4, owned, vec![7, 8, 9]), me).expect("defer owned");
    deferred.push((4, owned, vec![6]), me).expect("defer next owned");
    let unowned_tags = [unowned];
    let owned_tags = [owned];
    let arms = [Receive::Only(&unowned_tags), Receive::Only(&owned_tags)];
    let mut out = [0; 3];

    let got = crate::mpi::p2p::take_deferred(
        &deferred,
        me,
        Owner::new(&arms, 1),
        &mut out,
    )
    .expect("owned frame");
    assert_eq!(got, Some((4, owned, 3)));
    assert_eq!(&out, &[7, 8, 9]);
    let got = crate::mpi::p2p::take_deferred(
        &deferred,
        me,
        Owner::new(&arms, 1),
        &mut out,
    )
    .expect("next owned frame");
    assert_eq!(got, Some((4, owned, 1)));
    assert_eq!(out[0], 6);
    let got = crate::mpi::p2p::take_deferred(
        &deferred,
        me,
        Owner::new(&arms, 0),
        &mut out,
    )
    .expect("previously unowned frame");
    assert_eq!(got, Some((3, unowned, 2)));
    assert_eq!(&out[..2], &[1, 2]);
    let got = crate::mpi::p2p::take_deferred(
        &deferred,
        me,
        Owner::new(&arms, 0),
        &mut out,
    )
    .expect("next unowned frame");
    assert_eq!(got, Some((3, unowned, 1)));
    assert_eq!(out[0], 3);
    assert_eq!(
        crate::mpi::p2p::take_deferred(&deferred, me, Owner::ALL, &mut out)
            .expect("no duplicate frame"),
        None
    );
}

#[test]
fn an_edge_whose_endpoint_is_not_a_worker_is_refused() {
    // Not a panic and not a silent skip: the endpoint has no place in the worker list, so the
    // table cannot name it and the declaration is refused.
    let _ = HashMap::<(), ()>::new();
    assert!(crate::mpi::context::window(&[0, 1], &edges(&[(0, 9)]), 64).is_err());
}

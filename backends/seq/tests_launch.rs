//! Launch checks (spec.md 4): a topology that breaks one is refused, and nothing runs.

extern crate std;

use super::tests::{within_10s, Ping};
use crate::launch::{Refused, MAX_FRAME};
use crate::topology::{Context, Topology, WorkerId, LEADER};

fn quick(_cx: &Context) {}

/// Messages of exactly `MAX_FRAME + 1` and `MAX_FRAME` bytes of plain data.
#[crate::message]
struct Over([u8; MAX_FRAME + 1]);

#[crate::message]
struct AtMax([u8; MAX_FRAME]);

#[test]
fn duplicate_sender_receiver_tag_is_refused() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(quick);
        g.route::<Ping>(w, LEADER, 1);
        g.route::<Ping>(w, LEADER, 4);
        let refused = g.launch().err();
        assert_eq!(refused, Some(Refused::Duplicate { from: w, to: LEADER, name: core::any::type_name::<Ping>() }));
    });
}

#[test]
fn zero_capacity_is_refused() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(quick);
        g.route::<Ping>(w, LEADER, 0);
        assert!(matches!(g.launch(), Err(Refused::ZeroCapacity { .. })));
    });
}

#[test]
fn route_to_a_worker_not_in_the_topology_is_refused() {
    within_10s(|| {
        let mut g = Topology::new();
        g.worker(quick);
        g.route::<Ping>(LEADER, WorkerId(7), 1);
        assert_eq!(g.launch().err(), Some(Refused::UnknownWorker(WorkerId(7))));
    });
}

#[test]
fn message_over_the_frame_limit_is_refused_and_the_limit_is_accepted() {
    within_10s(|| {
        let mut g = Topology::new();
        g.route::<Over>(LEADER, LEADER, 1);
        assert!(matches!(g.launch(), Err(Refused::TooLarge { size, .. }) if size == MAX_FRAME + 1));

        let mut g = Topology::new();
        g.route::<AtMax>(LEADER, LEADER, 1);
        let run = g.launch().expect("a message of exactly MAX bytes is accepted");
        assert!(run.join().is_empty(), "no workers, so no results");
    });
}

#[test]
fn same_type_on_opposite_routes_is_not_a_duplicate() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(quick);
        g.route::<Ping>(w, LEADER, 1);
        g.route::<Ping>(LEADER, w, 1);
        assert_eq!(g.launch().expect("accepted").join().len(), 1);
    });
}

//! Launch checks (spec.md 4): a topology that breaks one is refused, and nothing runs.

extern crate std;

use super::tests::{Ping, within_10s};
use crate::io;
use crate::launch::Refused;
use crate::topology::{Context, LEADER, Topology, WorkerId};

fn quick(_cx: &Context) {}

/// A message of 256 KiB: no size limit applies, so it is one frame and launches (spec.md 4).
#[crate::message]
#[derive(Debug, PartialEq, Eq)]
struct Large([u8; 256 * 1024]);

fn send_large(_cx: &Context) {
    let mut bytes = [0u8; 256 * 1024];
    bytes[256 * 1024 - 1] = 7;
    while io::send(LEADER, Large(bytes)).is_err() {}
}

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
fn a_message_of_any_size_is_one_frame_and_round_trips() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker(send_large);
        g.route::<Large>(w, LEADER, 1);
        let run = g.launch().expect("no size limit: accepted");
        let got = super::tests::await_frame::<Large>(&run);
        assert_eq!(got.0[256 * 1024 - 1], 7);
        assert_eq!(run.join().len(), 1);
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

//! Launch checks (spec.md 4): a topology that breaks one is refused, and nothing runs.

extern crate std;

use super::tests::{Ping, Quick, await_frame, within_10s};
use crate::io;
use crate::launch::Refused;
use crate::status::Status;
use crate::topology::{LEADER, Topology, WorkerId};

/// A message of 256 KiB: no size limit applies, so it is one frame and launches (spec.md 4).
#[crate::message]
#[derive(Debug, PartialEq, Eq)]
struct Large([u8; 256 * 1024]);

#[crate::state(steps(send_large))]
struct LargeSender {}

#[crate::step(LargeSender)]
fn send_large() -> Status {
    let mut bytes = [0u8; 256 * 1024];
    bytes[256 * 1024 - 1] = 7;
    match io::send(LEADER, Large(bytes)) {
        Ok(()) => Status::Done,
        Err(_) => Status::wait().room::<Large>(LEADER),
    }
}

/// A worker of `Quick`, set.
fn quick_worker(g: &mut Topology) -> WorkerId {
    let w = g.worker::<Quick>();
    g.set(&w, || Quick {});
    w.id()
}

#[test]
fn duplicate_sender_receiver_tag_is_refused() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = quick_worker(&mut g);
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
        let w = quick_worker(&mut g);
        g.route::<Ping>(w, LEADER, 0);
        assert!(matches!(g.launch(), Err(Refused::ZeroCapacity { .. })));
    });
}

#[test]
fn route_to_a_worker_not_in_the_topology_is_refused() {
    within_10s(|| {
        let mut g = Topology::new();
        quick_worker(&mut g);
        g.route::<Ping>(LEADER, WorkerId::here(7), 1);
        assert_eq!(g.launch().err(), Some(Refused::UnknownWorker(WorkerId::here(7))));
    });
}

#[test]
fn a_seq_deployment_is_one_host_and_a_route_to_another_host_is_refused() {
    within_10s(|| {
        use crate::HostId;
        assert_eq!(crate::host(), HostId(0));
        assert_eq!(WorkerId::from((HostId(0), 3)), WorkerId::here(3));
        let mut g = Topology::new();
        let w = quick_worker(&mut g);
        g.route::<Ping>(w, (HostId(1), 0), 1);
        assert_eq!(g.launch().err(), Some(Refused::UnknownWorker(WorkerId(0, 1))));
    });
}

#[test]
fn a_message_of_any_size_is_one_frame_and_round_trips() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<LargeSender>();
        g.set(&w, || LargeSender {});
        g.route::<Large>(w.id(), LEADER, 1);
        let run = g.launch().expect("no size limit: accepted");
        let got = await_frame::<Large>(&run);
        assert_eq!(got.0[256 * 1024 - 1], 7);
        assert_eq!(run.join().len(), 1);
    });
}

#[test]
fn same_type_on_opposite_routes_is_not_a_duplicate() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = quick_worker(&mut g);
        g.route::<Ping>(w, LEADER, 1);
        g.route::<Ping>(LEADER, w, 1);
        assert_eq!(g.launch().expect("accepted").join().len(), 1);
    });
}

#[crate::message]
#[derive(Debug, PartialEq, Eq)]
struct Job {
    id: u32,
}

/// Two steps that receive the same type: one tag, two owners in one worker.
#[crate::state(steps(own1, own2))]
struct TwoOwn {}

#[crate::step(TwoOwn, receives = Job)]
fn own1() -> Status {
    Status::Done
}

#[crate::step(TwoOwn, receives = Job)]
fn own2() -> Status {
    Status::Done
}

#[test]
fn two_steps_receiving_one_type_are_refused() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<TwoOwn>();
        g.set(&w, || TwoOwn {});
        let refused = g.launch().err();
        assert_eq!(refused, Some(Refused::TwoOwners { worker: w.id(), name: core::any::type_name::<Job>() }));
    });
}

#[test]
fn a_worker_declared_but_never_set_is_refused() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Quick>();
        assert_eq!(g.launch().err(), Some(Refused::Unset { worker: w.id() }));
    });
}

/// The pool a step reads.
struct Config {
    #[allow(dead_code)]
    n: u32,
}

#[crate::state(steps(read_pool), pool = Config)]
struct Reader {}

#[crate::step(Reader)]
fn read_pool(pool: &Config) -> Status {
    let _ = pool.n;
    Status::Done
}

#[test]
fn a_pool_declared_but_never_set_is_refused() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Reader>();
        g.set(&w, || Reader {});
        let _pool = g.pool::<Config>();
        assert_eq!(g.launch().err(), Some(Refused::PoolUnset));
    });
}

#[test]
fn a_worker_that_reads_a_pool_the_topology_lacks_is_refused() {
    within_10s(|| {
        let mut g = Topology::new();
        let w = g.worker::<Reader>();
        g.set(&w, || Reader {});
        let refused = g.launch().err();
        assert_eq!(
            refused,
            Some(Refused::PoolType { worker: w.id(), want: core::any::type_name::<Config>(), have: "()" })
        );
    });
}

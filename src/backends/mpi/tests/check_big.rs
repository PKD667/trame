// The route size check. No size here is a limit the backend enforces: a route of 256 KiB carries one
// message, whole, whatever the node split. Run with no node split and with TRAME_MPI_NODE_RANKS = 1 and 2
// (run.sh).

#![allow(unused_imports)]

use crate::common::*;

use mpi::traits::*;
use trame::__mpi::{Answer, Received, Route, RouteId, Topology, leader_join, returned};

/// The message type's size: 256 KiB.
const ROUTE: usize = 256 * 1024;

/// The bytes of the message, `len` long: not a repeated byte, so a short, shifted or stale copy does not match.
fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 56) as u8 ^ 9u8.wrapping_mul(37)).collect()
}

pub(crate) fn big(world: &mpi::topology::SimpleCommunicator, _args: &[String]) {
    let me = world.rank() as u32;
    let n = world.size() as u32 - 1;
    assert!(n == 2, "bigroute wants two workers and the leader: three ranks");
    let route = Route { from: 0, to: 1, tag: 0, size: ROUTE, cap: 2, lossy: false, remote: None };
    let topo = Topology { workers: n, routes: vec![route] };
    let mut t = match worker(world, &topo) {
        Ok(t) => t,
        Err(l) => return leader_join(&l),
    };
    // Worker 0 sends one 256 KiB message; worker 1 receives it whole and checks every byte.
    if me == 0 {
        assert_eq!(t.send(RouteId(0), &pattern(ROUTE)).expect("send"), Answer::Ok, "route 0 is full");
    }
    if me == 1 {
        let mut buf = vec![0u8; ROUTE];
        let id = loop {
            // A receive that found nothing is retried: with several nodes the frame may still be in transit.
            match t.recv(&[RouteId(0)], &mut buf).expect("recv") {
                Received::Message(id) => break id,
                Received::Empty => std::hint::spin_loop(),
            }
        };
        assert_eq!(id.0, 0, "the message came on route 0");
        assert!(buf == pattern(ROUTE), "the 256 KiB message arrived with wrong bytes");
        println!("bigroute: a {ROUTE}-byte message delivered");
    }
    returned(&t);
}

// The publication and route size check. No size here is a limit the backend enforces: a route of 256 KiB
// carries one message, then the leader publishes 1 MiB, 3 bytes and 4 MiB, and every worker reads each one
// back after a barrier. Run with no node split and with TRAME_MPI_NODE_RANKS = 1 and 2 (run.sh).

#![allow(unused_imports)]

use crate::common::*;

use mpi::traits::*;
use trame::__mpi::{Answer, Received, Route, RouteId, Topology, leader_join, returned};

/// The message type's size: 256 KiB.
const ROUTE: usize = 256 * 1024;
/// The publications the leader makes, in order, in bytes.
const SIZES: [usize; 3] = [1 << 20, 3, 4 << 20];

/// The bytes of round `k`, `len` long: distinct per round, and not a repeated byte, so a short, shifted or
/// stale copy does not match.
fn pattern(k: usize, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 56) as u8 ^ (k as u8).wrapping_mul(37))
        .collect()
}

pub(crate) fn publication(world: &mpi::topology::SimpleCommunicator, _args: &[String]) {
    let me = world.rank() as u32;
    let n = world.size() as u32 - 1;
    assert!(n == 2, "publication wants two workers and the leader: three ranks");
    let route = Route { from: 0, to: 1, tag: 0, size: ROUTE, cap: 2, lossy: false };
    let topo = Topology { workers: n, routes: vec![route], publication: Vec::new() };
    let mut t = match worker(world, &topo) {
        Ok(t) => t,
        Err(mut l) => {
            world.barrier();
            for (k, &len) in SIZES.iter().enumerate() {
                // The deposit is complete before the first barrier, so the workers' barrier that fires
                // after it installs it. The second barrier waits until every worker has read it: a deposit
                // made before that would replace it (latest wins, spec 5.5).
                l.publish(&pattern(k, len));
                world.barrier();
                world.barrier();
            }
            return leader_join(&l);
        }
    };
    // The route: worker 0 sends one 256 KiB message, worker 1 receives it whole and checks every byte.
    if me == 0 {
        let msg = pattern(9, ROUTE);
        assert_eq!(t.send(RouteId(0), &msg).expect("send"), Answer::Ok, "route 0 is full");
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
        assert!(buf == pattern(9, ROUTE), "the 256 KiB message arrived with wrong bytes");
    }
    world.barrier();
    for (k, &len) in SIZES.iter().enumerate() {
        world.barrier();
        let got = t.barrier().expect("barrier");
        let want = pattern(k, len);
        let first = got.iter().zip(&want).position(|(a, b)| a != b);
        assert!(
            got.len() == len && first.is_none(),
            "rank {me}: publication {k} of {len} bytes read back as {} bytes, first difference at {first:?}",
            got.len()
        );
        world.barrier();
    }
    if me == 0 {
        println!("publication: {SIZES:?} bytes read back by both workers; a {ROUTE}-byte message delivered");
    }
    returned(&t);
}

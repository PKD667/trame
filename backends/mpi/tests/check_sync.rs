// The barrier and fair checks.

#![allow(unused_imports)]

use crate::common::*;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mpi::traits::*;
use trame::__mpi::{clock, deadlocked, launch, leader_join, returned, Answer, Error, Event, Received, Role, Route, RouteId, Topology, Transport};

pub(crate) fn barrier(world: &mpi::topology::SimpleCommunicator, args: &[String]) {
    let seed = arg_num(args, "--seed", 1);
    let rounds = arg_num(args, "--rounds", 200);
    let me = world.rank() as u32;
    let n = world.size() as u32 - 1;
    let topo = Topology { workers: n, routes: Vec::new(), publication: b"init".to_vec() };
    let mut t = match worker(world, &topo) {
        Ok(t) => t,
        Err(mut l) => {
            // One deposit per round. `publish` never waits, so later deposits replace earlier ones that
            // no barrier installed yet.
            for k in 1..=rounds {
                l.publish(format!("round {k}").as_bytes());
            }
            // The window lives in this rank: stay until every worker is done with it.
            world.barrier();
            return leader_join(&l);
        }
    };
    let mut rng = Rng::new(seed ^ u64::from(me));
    let last = format!("round {rounds}").into_bytes();
    // Distinct values in the order this worker read them. The publication changes only at a
    // barrier, so a value is read at one barrier or more and never skipped.
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut calls = 0u64;
    // Keep meeting the others until the last deposit is read. A deposit is applied at a barrier
    // that completes after it, so the last one may need barriers after the leader has finished.
    while seen.last() != Some(&last) {
        calls += 1;
        assert!(calls <= 100 * rounds + 1000, "rank {me}: the last publication never arrived");
        rng.spin(2000);
        let got = t.barrier().expect("barrier").to_vec();
        if seen.last() != Some(&got) {
            seen.push(got);
        }
    }
    // Spec 5.5: a barrier answers the publication current when it fires, so a worker sees `init`
    // only if it reaches a barrier before the leader's first deposit is installed, and then rounds in
    // order, each at most once. The leader's deposits race the workers' barriers, so the exact
    // subsequence depends on timing; the order and the final value do not.
    let mut want_round = 0u64;
    for v in &seen {
        if v == b"init" {
            assert_eq!(want_round, 0, "rank {me}: init seen after a round");
            continue;
        }
        let k: u64 = std::str::from_utf8(v).expect("utf8").trim_start_matches("round ").parse().expect("round");
        assert!(k > want_round, "rank {me}: publication {k} out of order after {want_round}");
        want_round = k;
    }
    assert_eq!(want_round, rounds, "rank {me}: the last publication is not the final round");
    world.barrier();
    println!("barrier rank {me}: {rounds} rounds, publications seen in order, the last one last");
    returned(&t);
}

/// Workers 0 and 1 each send `msgs` frames to worker 2, on its own route, before the receiver
/// starts. Both routes then hold frames until the first one is drained, so a rotating receiver
/// must take them in turns: it may not take the same route twice in a row while both have frames.
pub(crate) fn fair(world: &mpi::topology::SimpleCommunicator, args: &[String]) {
    let msgs = arg_num(args, "--msgs", 16);
    let me = world.rank() as u32;
    assert!(world.size() == 4, "fair wants two senders, a receiver and the leader: four ranks");
    let route = |from| Route { from, to: 2, tag: 0, size: 64, cap: msgs as u32, lossy: false };
    let topo = Topology { workers: 3, routes: vec![route(0), route(1)], publication: Vec::new() };
    let mut t = match worker(world, &topo) {
        Ok(t) => t,
        Err(l) => {
            world.barrier();
            world.barrier();
            return leader_join(&l);
        }
    };
    let mut msg = [0u8; 64];
    if me < 2 {
        for i in 0..msgs {
            fill(me, i, &mut msg);
            assert_eq!(t.send(RouteId(me), &msg).expect("send"), Answer::Ok, "route {me} is not full");
        }
    }
    world.barrier();
    if me != 2 {
        world.barrier();
        returned(&t);
        return;
    }
    let owned = [RouteId(0), RouteId(1)];
    let mut buf = [0u8; 64];
    let mut order = Vec::new();
    // One node: every frame is in shared memory before the barrier, so both routes hold frames at the
    // start and the rotation is checked. Several nodes: a frame may still be in transit, which the
    // spec allows (a frame is available only from its arrival time, spec 5.2). Then the receiver
    // waits for each frame, and checks what the spec promises: every route is FIFO.
    let several = std::env::var_os("TRAME_MPI_NODE_RANKS").is_some();
    let mut next = [0u64; 2];
    for _ in 0..2 * msgs {
        let id = loop {
            match t.recv(&owned, &mut buf).expect("recv") {
                Received::Message(id) => break id,
                Received::Empty if several => std::hint::spin_loop(),
                Received::Empty => panic!("receiver found both routes empty before {} frames", 2 * msgs),
            }
        };
        check_frame(id.0, next[id.0 as usize], &buf);
        next[id.0 as usize] += 1;
        order.push(id.0);
    }
    if !several {
        for (i, pair) in order.windows(2).enumerate() {
            assert_ne!(pair[0], pair[1], "receives {i} and {} both took route {}", i + 1, pair[0]);
        }
    }
    println!("fair rank 2: {msgs} frames per route, taken in turns: {order:?}");
    world.barrier();
    returned(&t);
}

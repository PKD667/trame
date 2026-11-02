// The fair check.

#![allow(unused_imports)]

use crate::common::*;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mpi::traits::*;
use trame::__mpi::{
    Answer, Error, Event, Received, Role, Route, RouteId, Topology, Transport, clock, launch, leader_join,
    returned,
};

/// Workers 0 and 1 each send `msgs` frames to worker 2, on its own route, before the receiver
/// starts. Both routes then hold frames until the first one is drained, so a rotating receiver
/// must take them in turns: it may not take the same route twice in a row while both have frames.
pub(crate) fn fair(world: &mpi::topology::SimpleCommunicator, args: &[String]) {
    let msgs = arg_num(args, "--msgs", 16);
    let me = world.rank() as u32;
    assert!(world.size() == 4, "fair wants two senders, a receiver and the leader: four ranks");
    let route = |from| Route { from, to: 2, tag: 0, size: 64, cap: msgs as u32, lossy: false };
    let topo = Topology { workers: 3, routes: vec![route(0), route(1)] };
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

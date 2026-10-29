// The overwrite and deadlock checks.

#![allow(unused_imports)]

use crate::common::*;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mpi::traits::*;
use trame::__mpi::{
    Answer, Error, Event, Received, Role, Route, RouteId, Topology, Transport, clock, deadlocked, launch, leader_join,
    returned,
};

pub(crate) fn overwrite(world: &mpi::topology::SimpleCommunicator, args: &[String]) {
    let seed = arg_num(args, "--seed", 1);
    let cap = arg_num(args, "--cap", 4) as u32;
    let msgs = arg_num(args, "--msgs", 5000);
    let late = args.iter().any(|a| a == "--late");
    let me = world.rank() as u32;
    let n = world.size() as u32 - 1;
    assert!(n >= 2, "overwrite wants two workers");
    let topo = Topology {
        workers: n,
        routes: vec![Route { from: 0, to: 1, tag: 0, size: 64, cap, lossy: true }],
        publication: Vec::new(),
    };
    let mut t = match worker(world, &topo) {
        Ok(t) => t,
        Err(l) => {
            // The leader joins the two harness barriers, then waits for the workers' results.
            if late {
                world.barrier();
                world.barrier();
            }
            return leader_join(&l);
        }
    };
    let mut buf = vec![0u8; FRAME];
    let r = RouteId(0);
    if me == 0 {
        let mut rng = Rng::new(seed);
        let mut msg = [0u8; 64];
        for i in 0..msgs {
            fill(0, i, &mut msg);
            let a = t.send(r, &msg).expect("send");
            assert_eq!(a, Answer::Ok, "lossy send answered Full at {i}");
            rng.spin(200);
        }
        println!("overwrite rank 0: sent {msgs} lossy frames, all Ok");
    }
    if late {
        // The harness, not the transport, makes the receiver wait for the sender to finish.
        world.barrier();
    }
    if me == 1 {
        let owned = [r];
        let last = msgs - 1;
        if late {
            // Every frame has been sent, but a send only queues it: the frames reach this node later (spec.md 6,
            // Progress: "eventually"), so a barrier does not bound it. Receive until the newest frame arrives: the
            // frames seen are increasing, and the newest is kept.
            let mut got: Vec<u64> = Vec::new();
            while got.last() != Some(&last) {
                if let Received::Message(_) = t.recv(&owned, &mut buf).expect("recv") {
                    got.push(u64::from_le_bytes(buf[4..12].try_into().unwrap()));
                }
            }
            assert!(got.windows(2).all(|w| w[0] < w[1]), "late read reordered: {got:?}");
            println!("overwrite rank 1 (late): {} of {msgs} frames seen, increasing, newest {last} kept", got.len());
        } else {
            // Concurrent with the sender: the frames seen are increasing and the newest is kept.
            let mut prev: Option<u64> = None;
            let mut count = 0u64;
            let mut last_seen = None;
            while last_seen != Some(last) {
                if let Received::Message(_) = t.recv(&owned, &mut buf).expect("recv") {
                    let seq = u64::from_le_bytes(buf[4..12].try_into().unwrap());
                    if let Some(p) = prev {
                        assert!(seq > p, "lossy route reordered: {seq} after {p}");
                    }
                    prev = Some(seq);
                    last_seen = Some(seq);
                    count += 1;
                }
            }
            println!("overwrite rank 1: {count} of {msgs} frames seen, increasing, newest {last} kept");
        }
    }
    if late {
        world.barrier();
    }
    returned(&t);
}

pub(crate) fn deadlock(world: &mpi::topology::SimpleCommunicator) {
    let me = world.rank() as u32;
    let n = world.size() as u32 - 1;
    assert!(n >= 2, "deadlock wants two workers");
    let topo = Topology { workers: n, routes: Vec::new(), publication: Vec::new() };
    let t = match worker(world, &topo) {
        Ok(t) => t,
        Err(l) => return leader_join(&l),
    };
    if me == 0 {
        // Return while the others wait at the barrier. Dropping the transport must not wait.
        println!("deadlock rank 0: returned at {}", clock());
        returned(&t);
        drop(t);
        return;
    }
    if me == 1 {
        // Keep working for two seconds first, so the barrier is still waited on when rank 0 leaves.
        let until = clock() + 2_000_000_000;
        while clock() < until {
            std::hint::spin_loop();
        }
    }
    let mut t = t;
    match t.barrier() {
        Err(Error::Deadlock) => {
            println!("deadlock rank {me}: Deadlock at {}", clock());
            deadlocked(&t);
        }
        other => panic!("rank {me}: expected Deadlock, got {other:?}"),
    }
}

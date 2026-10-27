// The fifo check: random routes and interleavings, with a log for verify.

#![allow(unused_imports)]

use crate::common::*;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mpi::traits::*;
use trame::__mpi::{clock, deadlocked, launch, leader_join, returned, Answer, Error, Event, Received, Role, Route, RouteId, Topology, Transport};

pub(crate) fn fifo(world: &mpi::topology::SimpleCommunicator, args: &[String]) {
    let seed = arg_num(args, "--seed", 1);
    let max = arg_num(args, "--msgs", 300);
    let dir = PathBuf::from(arg(args, "--dir").expect("fifo wants --dir"));
    fs::create_dir_all(&dir).expect("log dir");
    let me = world.rank() as u32;
    let n = world.size() as u32 - 1;
    let topo = random_topology(seed, n);
    let mut t = match worker(world, &topo) {
        Ok(t) => t,
        Err(l) => return leader_join(&l),
    };
    if me == 0 {
        let mut s = String::new();
        for (i, r) in topo.routes.iter().enumerate() {
            s += &format!("R {i} {} {} {} {} {}\n", r.from, r.to, r.size, r.cap, u8::from(r.lossy));
        }
        fs::write(dir.join("topo.txt"), s).expect("topo");
    }
    t.trace_on();

    let counts = plan_counts(seed, topo.routes.len(), max);
    let owned = ids_to(&topo.routes, me);
    let mut tokens: Vec<u32> = Vec::new();
    for (r, route) in topo.routes.iter().enumerate() {
        if route.from == me {
            tokens.extend(std::iter::repeat_n(r as u32, counts[r] as usize));
        }
    }
    let mut rng = Rng::new(seed ^ (u64::from(me) << 32) ^ 0xf1f0);
    for i in (1..tokens.len()).rev() {
        tokens.swap(i, rng.below(i as u64 + 1) as usize);
    }
    let expected_in: u64 = topo
        .routes
        .iter()
        .enumerate()
        .filter(|(_, r)| r.to == me)
        .map(|(i, _)| counts[i])
        .sum();

    let mut next = 0usize;
    let mut sent_seq = vec![0u64; topo.routes.len()];
    let mut recv_seq = vec![0u64; topo.routes.len()];
    let mut received = 0u64;
    let mut fulls = 0u64;
    let mut msg = vec![0u8; FRAME];
    let mut buf = vec![0u8; FRAME];
    while next < tokens.len() || received < expected_in {
        match rng.below(3) {
            0 if next < tokens.len() => {
                let r = tokens[next];
                let size = topo.routes[r as usize].size;
                fill(r, sent_seq[r as usize], &mut msg[..size]);
                match t.send(RouteId(r), &msg[..size]).expect("send") {
                    Answer::Ok => {
                        sent_seq[r as usize] += 1;
                        next += 1;
                    }
                    Answer::Full => fulls += 1,
                }
            }
            _ => {
                if let Received::Message(id) = t.recv(&owned, &mut buf).expect("recv") {
                    let r = id.0 as usize;
                    check_frame(id.0, recv_seq[r], &buf[..topo.routes[r].size]);
                    recv_seq[r] += 1;
                    received += 1;
                }
            }
        }
        rng.spin(40);
    }
    write_events(&dir, me, &t.take_trace());
    println!("fifo rank {me}: sent {} received {} full {fulls}", tokens.len(), received);
    returned(&t);
}

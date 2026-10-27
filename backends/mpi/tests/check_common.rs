// Helpers for the MPI checks: launch, seeded generator, frame payloads, logs.

#![allow(unused_imports)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mpi::traits::*;
use trame::__mpi::{clock, deadlocked, launch, leader_join, returned, Answer, Error, Event, Received, Role, Route, RouteId, Topology, Transport, MAX};

/// The worker transport of this rank, or None for the leader (the last rank).
/// This rank's endpoint: a worker's, or the leader's (Err).
pub(crate) fn worker(world: &mpi::topology::SimpleCommunicator, topo: &Topology) -> Result<Transport, Transport> {
    match launch(world, topo).unwrap_or_else(|e| panic!("refused: {e}")) {
        Role::Worker(t) => Ok(t),
        Role::Leader(l) => Err(l),
    }
}

/// A small seeded generator (xorshift64*). Each rank derives its own stream from the seed.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    /// Busy work of a random length: this is how the interleavings vary, never a sleep.
    pub(crate) fn spin(&mut self, max: u64) {
        for _ in 0..self.below(max + 1) {
            std::hint::spin_loop();
        }
    }
}

pub(crate) fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

pub(crate) fn arg_num(args: &[String], name: &str, default: u64) -> u64 {
    arg(args, name).map_or(default, |v| v.parse().unwrap_or_else(|_| panic!("{name} wants a number")))
}

/// A random topology of `n` workers, from the seed alone, so every rank derives the same one.
pub(crate) fn random_topology(seed: u64, n: u32) -> Topology {
    let mut rng = Rng::new(seed ^ 0x51);
    let mut routes: Vec<Route> = Vec::new();
    for _ in 0..(3 * n + 2) {
        let from = rng.below(u64::from(n)) as u32;
        let to = rng.below(u64::from(n)) as u32;
        let tag = rng.below(3);
        if routes.iter().any(|r| (r.from, r.to, r.tag) == (from, to, tag)) {
            continue;
        }
        let size = [16, 200, MAX][rng.below(3) as usize];
        let cap = 1 + rng.below(8) as u32;
        routes.push(Route { from, to, tag, size, cap, lossy: false });
    }
    Topology { workers: n, routes, publication: b"init".to_vec() }
}

/// How many frames each route carries, from the seed and the route's rank only.
pub(crate) fn plan_counts(seed: u64, routes: usize, max: u64) -> Vec<u64> {
    (0..routes)
        .map(|r| Rng::new(seed ^ (r as u64).wrapping_mul(0x1000_0000_01b3) ^ 0xc0)
            .below(max + 1))
        .collect()
}

pub(crate) fn ids_to(routes: &[Route], me: u32) -> Vec<RouteId> {
    routes
        .iter()
        .enumerate()
        .filter(|(_, r)| r.to == me)
        .map(|(i, _)| RouteId(i as u32))
        .collect()
}

pub(crate) fn fill(route: u32, seq: u64, buf: &mut [u8]) {
    buf[0..4].copy_from_slice(&route.to_le_bytes());
    buf[4..12].copy_from_slice(&seq.to_le_bytes());
    for (k, b) in buf[12..].iter_mut().enumerate() {
        *b = (seq as u8).wrapping_add(k as u8);
    }
}

/// Check a received message's header and payload. Panics on any mismatch.
pub(crate) fn check_frame(route: u32, seq: u64, buf: &[u8]) {
    let got_route = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    let got_seq = u64::from_le_bytes(buf[4..12].try_into().unwrap());
    assert_eq!(got_route, route, "frame on route {route} carries route {got_route}");
    assert_eq!(got_seq, seq, "route {route}: FIFO broken, expected seq {seq}, got {got_seq}");
    for (k, b) in buf[12..].iter().enumerate() {
        assert_eq!(*b, (seq as u8).wrapping_add(k as u8), "route {route} seq {seq}: payload byte {k}");
    }
}

pub(crate) fn write_events(dir: &Path, me: u32, events: &[Event]) {
    let mut f = fs::File::create(dir.join(format!("rank-{me}.log"))).expect("create log");
    for e in events {
        match *e {
            Event::Send { route, index, read, before, after, answer } => writeln!(
                f,
                "S {route} {index} {read} {before} {after} {}",
                if answer == Answer::Ok { 0 } else { 1 }
            ),
            Event::Consume { route, index, before, after } => {
                writeln!(f, "C {route} {index} {before} {after}")
            }
        }
        .expect("write log");
    }
}

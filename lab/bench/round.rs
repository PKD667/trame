//! bench_round W ROUNDS: W workers and a round of messages through the leader. There is no barrier in the
//! worker model, so this is the nearest thing it can measure: how long W workers take to be brought in step
//! by the one agent that can do it. The leader sends `Go(r)` to every worker; a worker answers `Ack(r)` when
//! its step takes it; the leader sends `Go(r + 1)` only when all W acks of round r are in. Round 0 is a
//! warm-up. A worker starts its clock when it sends `Ack(0)` and stops it when it sends `Ack(ROUNDS)`, so
//! its span is ROUNDS full rounds (ack, the leader's gather, the release, ack) on its own clock. It then
//! sends one Report. The leader prints one JSON line per worker, then the outcomes.
//!
//! This is not a library barrier's measurement: a barrier is W ranks agreeing among themselves, and this is W
//! workers and a leader (W + 1 agents) agreeing through the leader, with 2 x W messages a round. The plain-process
//! baseline (barrier.c, in a backend's bench directory) is still ROUNDS library barriers on W processes, the cost of the same
//! synchronisation done by the library. Built with `--cfg trame_checked` (flake.nix sets it).

#[path = "common.rs"]
mod common;

use trame::{Leader, Reading, Status, Topology, Worker};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Go {
    round: u64,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Ack {
    worker: u64,
    round: u64,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u64,
    ns: u64,
}

/// `next` is the round whose `Go` the worker takes next; `owes` is set from taking a `Go` until its
/// `Ack` is sent, so a full route to the leader loses nothing.
#[trame::state(steps(pace))]
struct Pace {
    me: u64,
    rounds: u64,
    next: u64,
    owes: bool,
    t0: Option<Reading>,
    ns: Option<u64>,
}

/// Answers each `Go` with an `Ack` of its round, times rounds 1..=ROUNDS, then reports once.
#[trame::step(Pace, receives = Go)]
fn pace(
    me: &u64,
    rounds: &u64,
    next: &mut u64,
    owes: &mut bool,
    t0: &mut Option<Reading>,
    ns: &mut Option<u64>,
) -> Status {
    if let Some(ns) = *ns {
        return match trame::send(Leader, Report { worker: *me, ns }) {
            Ok(()) => Status::Done,
            Err(_) => Status::wait().room::<Report>(Leader),
        };
    }
    if !*owes {
        match trame::recv::<Go>() {
            Some(g) => {
                assert_eq!(g.round, *next, "a go out of order");
                *owes = true;
            }
            None => return Status::wait().frame(),
        }
    }
    if trame::send(Leader, Ack { worker: *me, round: *next }).is_err() {
        return Status::wait().room::<Ack>(Leader);
    }
    *owes = false;
    if *next == 0 {
        *t0 = Some(trame::now());
    }
    if *next == *rounds {
        let start = t0.expect("the warm-up round ended");
        *ns = Some(common::ns(trame::now().since(start)));
    }
    *next += 1;
    Status::Progress
}

fn main() {
    let w: usize = common::arg(1, "W");
    let rounds: u64 = common::arg(2, "ROUNDS");
    if w == 0 {
        eprintln!("usage: W must be at least 1");
        std::process::exit(2);
    }
    let mut t = Topology::new();
    let ws: Vec<Worker<Pace>> = (0..w).map(|_| t.worker()).collect();
    for (i, h) in ws.iter().enumerate() {
        t.set(h, move || Pace { me: i as u64, rounds, next: 0, owes: false, t0: None, ns: None });
        t.route::<Go>(Leader, h.id(), 1);
        t.route::<Ack>(h.id(), Leader, 1);
        t.route::<Report>(h.id(), Leader, 1);
    }
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    for round in 0..=rounds {
        for h in &ws {
            // Every ack of the last round is in before this go, and a worker sends its ack after taking its go.
            run.send(h.id(), Go { round }).expect("the go did not fit");
        }
        common::collect::<Ack>(&run, w, |a| assert_eq!(a.round, round, "an ack of another round"));
    }
    common::collect::<Report>(&run, w, |r| {
        println!("{{\"bench\":\"round\",\"workers\":{w},\"worker\":{},\"rounds\":{rounds},\"ns\":{}}}", r.worker, r.ns);
    });
    common::finish(run);
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

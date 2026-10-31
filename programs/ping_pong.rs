//! ping_pong: worker 0 and worker 1 trade 1000 round trips. Worker 0 reports the count to the leader,
//! which prints it and then the outcomes. Built with `--cfg trame_checked`, because `#[trame::state]`
//! needs it (run.sh sets it). Expected output: ping_pong.expected.

use trame::{LEADER, Outcome, Status, Topology, WorkerId};

const ROUNDS: u32 = 1000;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Ping {
    n: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Pong {
    n: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Count {
    rounds: u32,
}

/// Worker 0: the next round, whether it awaits a `Pong`, whether it reported, and worker 1.
#[trame::state(steps(pinger))]
struct Pinger {
    next: u32,
    waiting: bool,
    reported: bool,
    to: WorkerId,
}

/// Sends `Ping n` for n = 0.., waits for each `Pong`, then reports once to the leader.
#[trame::step(Pinger, receives = Pong)]
fn pinger(next: &mut u32, waiting: &mut bool, reported: &mut bool, to: &WorkerId) -> Status {
    if *waiting {
        match trame::recv::<Pong>() {
            Some(p) => {
                assert_eq!(p.n, *next, "a pong out of order");
                *waiting = false;
                *next += 1;
            }
            None => return Status::wait().frame(),
        }
    }
    if *next == ROUNDS {
        if *reported {
            return Status::Done;
        }
        return match trame::send(LEADER, Count { rounds: *next }) {
            Ok(()) => {
                *reported = true;
                Status::Progress
            }
            Err(_) => Status::wait().room::<Count>(LEADER),
        };
    }
    match trame::send(*to, Ping { n: *next }) {
        Ok(()) => {
            *waiting = true;
            Status::Progress
        }
        Err(_) => Status::wait().room::<Ping>(*to),
    }
}

/// Worker 1: the next round, the `Pong` it still has to send, and worker 0.
#[trame::state(steps(ponger))]
struct Ponger {
    next: u32,
    reply: Option<u32>,
    to: WorkerId,
}

/// Answers each `Ping` with the `Pong` of the same number, ROUNDS times.
#[trame::step(Ponger, receives = Ping)]
fn ponger(next: &mut u32, reply: &mut Option<u32>, to: &WorkerId) -> Status {
    if let Some(n) = *reply {
        return match trame::send(*to, Pong { n }) {
            Ok(()) => {
                *reply = None;
                *next += 1;
                Status::Progress
            }
            Err(_) => Status::wait().room::<Pong>(*to),
        };
    }
    if *next == ROUNDS {
        return Status::Done;
    }
    match trame::recv::<Ping>() {
        Some(p) => {
            assert_eq!(p.n, *next, "a ping out of order");
            *reply = Some(p.n);
            Status::Progress
        }
        None => Status::wait().frame(),
    }
}

fn main() {
    let mut t = Topology::new();
    let ids = t.ids(2);
    let a = t.worker(Pinger { next: 0, waiting: false, reported: false, to: ids[1] });
    let b = t.worker(Ponger { next: 0, reply: None, to: ids[0] });
    t.route::<Ping>(a.id(), b.id(), 1);
    t.route::<Pong>(b.id(), a.id(), 1);
    t.route::<Count>(a.id(), LEADER, 1);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let count = loop {
        if let Some(c) = run.recv::<Count>() {
            break c;
        }
        run.wait();
    };
    println!("rounds: {}", count.rounds);
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

//! ping_pong: worker 0 and worker 1 trade 1000 round trips. Worker 0 reports the count to the leader,
//! which prints it and then the outcomes. Built with `--cfg trame_checked`, because `concurrent!`
//! needs it (run.sh sets it). Expected output: ping_pong.expected.

use trame::{Context, LEADER, Outcome, Status, Topology, WorkerId};

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

/// The cell the processes share. It holds nothing: the processes talk over routes.
struct Shared;

/// Worker 0: sends `Ping n` for n = 0.., waits for each `Pong`, then reports once to the leader.
struct Pinger {
    next: u32,
    waiting: bool,
    reported: bool,
    to: WorkerId,
}

#[trame::process(receives = Pong)]
impl Pinger {
    fn step(&mut self, _s: &mut Shared) -> Status {
        if self.waiting {
            match trame::recv::<Pong>() {
                Some(p) => {
                    assert_eq!(p.n, self.next, "a pong out of order");
                    self.waiting = false;
                    self.next += 1;
                }
                None => return Status::wait().frame(),
            }
        }
        if self.next == ROUNDS {
            if self.reported {
                return Status::Done;
            }
            return match trame::send(LEADER, Count { rounds: self.next }) {
                Ok(()) => {
                    self.reported = true;
                    Status::Progress
                }
                Err(_) => Status::wait().room::<Count>(LEADER),
            };
        }
        match trame::send(self.to, Ping { n: self.next }) {
            Ok(()) => {
                self.waiting = true;
                Status::Progress
            }
            Err(_) => Status::wait().room::<Ping>(self.to),
        }
    }
}

/// Worker 1: answers each `Ping` with the `Pong` of the same number, ROUNDS times.
struct Ponger {
    next: u32,
    reply: Option<u32>,
    to: WorkerId,
}

#[trame::process(receives = Ping)]
impl Ponger {
    fn step(&mut self, _s: &mut Shared) -> Status {
        if let Some(n) = self.reply {
            return match trame::send(self.to, Pong { n }) {
                Ok(()) => {
                    self.reply = None;
                    self.next += 1;
                    Status::Progress
                }
                Err(_) => Status::wait().room::<Pong>(self.to),
            };
        }
        if self.next == ROUNDS {
            return Status::Done;
        }
        match trame::recv::<Ping>() {
            Some(p) => {
                assert_eq!(p.n, self.next, "a ping out of order");
                self.reply = Some(p.n);
                Status::Progress
            }
            None => Status::wait().frame(),
        }
    }
}

fn pinger(cx: &Context) {
    let other = cx.workers()[1];
    let mut shared = Shared;
    let r = trame::concurrent!(Pinger { next: 0, waiting: false, reported: false, to: other }; shared);
    assert_eq!(r, Ok(()));
}

fn ponger(cx: &Context) {
    let other = cx.workers()[0];
    let mut shared = Shared;
    let r = trame::concurrent!(Ponger { next: 0, reply: None, to: other }; shared);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let a = t.worker(pinger);
    let b = t.worker(ponger);
    t.route::<Ping>(a, b, 1);
    t.route::<Pong>(b, a, 1);
    t.route::<Count>(a, LEADER, 1);
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

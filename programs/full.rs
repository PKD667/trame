//! full: every operation of the B1 surface in one run. The leader sends `Go` to worker 0. Worker 0 runs
//! two processes over one shared cell: `Starter` takes `Go` and opens the cell, and `Producer` waits on
//! that cell (`Status::wait()`) before it sends five jobs to worker 1 over a route of capacity 2 (so it
//! waits for room). Worker 1 turns each job into an event for the leader, and checks its clock. The
//! leader prints the events in order, then the outcomes.

use trame::{Context, LEADER, Outcome, Status, Topology, WorkerId};

const JOBS: u32 = 5;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Go {
    token: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Job {
    id: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Ev {
    v: u32,
}

/// The cell both processes of worker 0 share: `open` is set once `Go` has arrived.
struct Gate {
    open: bool,
}

/// Takes `Go` and opens the gate. It owns `Go`, so it wakes only for one.
struct Starter;

#[trame::process(receives = Go)]
impl Starter {
    fn step(&mut self, g: &mut Gate) -> Status {
        match trame::recv::<Go>() {
            Some(_) => {
                g.open = true;
                Status::Done
            }
            None => Status::wait().frame(),
        }
    }
}

/// Sends jobs 0.. to `to`, after the gate opens. A full route makes it wait for room.
struct Producer {
    next: u32,
    to: WorkerId,
}

#[trame::process]
impl Producer {
    fn step(&mut self, g: &mut Gate) -> Status {
        if !g.open {
            return Status::wait();
        }
        if self.next == JOBS {
            return Status::Done;
        }
        match trame::send(self.to, Job { id: self.next }) {
            Ok(()) => {
                self.next += 1;
                Status::Progress
            }
            Err(_) => Status::wait().room::<Job>(self.to),
        }
    }
}

/// Worker 1: turns each job into an event for the leader, in order.
struct Consumer {
    seen: u32,
    pending: Option<u32>,
}

#[trame::process(receives = Job)]
impl Consumer {
    fn step(&mut self, _g: &mut Gate) -> Status {
        if let Some(v) = self.pending {
            return match trame::send(LEADER, Ev { v }) {
                Ok(()) => {
                    self.pending = None;
                    Status::Progress
                }
                Err(_) => Status::wait().room::<Ev>(LEADER),
            };
        }
        if self.seen == JOBS {
            return Status::Done;
        }
        match trame::recv::<Job>() {
            Some(j) => {
                self.seen += 1;
                self.pending = Some(j.id * 10);
                Status::Progress
            }
            None => Status::wait().frame(),
        }
    }
}

fn worker0(cx: &Context) {
    let to = cx.workers()[1];
    let mut gate = Gate { open: false };
    let r = trame::concurrent!(Starter, Producer { next: 0, to }; gate);
    assert_eq!(r, Ok(()));
}

fn worker1(_cx: &Context) {
    // Worker 1's clock is the run's clock: a reading of it compares with a later one.
    let start = trame::now();
    let mut gate = Gate { open: true };
    let r = trame::concurrent!(Consumer { seen: 0, pending: None }; gate);
    assert_eq!(r, Ok(()));
    // `since` aborts when the readings come from different clocks, so this also checks the clock.
    let _elapsed = trame::now().since(start);
}

fn main() {
    let mut t = Topology::new();
    let a = t.worker(worker0);
    let b = t.worker(worker1);
    t.route::<Go>(LEADER, a, 1);
    t.route::<Job>(a, b, 2);
    t.route::<Ev>(b, LEADER, 8);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    assert!(run.send(a, Go { token: 1 }).is_ok(), "go did not fit");
    let mut events = Vec::new();
    while events.len() < JOBS as usize {
        match run.recv::<Ev>() {
            Some(e) => events.push(e.v),
            None => run.wait(),
        }
    }
    for v in events {
        println!("ev {v}");
    }
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

//! full: every operation of the B1 surface in one run. The leader sends `Go` to worker 0. Worker 0 has
//! two steps over one field: `starter` takes `Go` and opens the gate, and `producer` waits on the gate
//! (`Status::wait()`, woken when `starter` ends) before it sends five jobs to worker 1 over a route of
//! capacity 2 (so it waits for room). Worker 1 turns each job into an event for the leader, and checks
//! its clock. The leader prints the events in order, then the outcomes.

use trame::{LEADER, Outcome, Reading, Status, Topology, WorkerId};

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

/// Worker 0: `open` is set once `Go` has arrived; `next` is the next job; `to` is worker 1.
#[trame::state(steps(starter, producer))]
struct Source {
    open: bool,
    next: u32,
    to: WorkerId,
}

/// Takes `Go` and opens the gate. It receives `Go`, so it wakes only for one.
#[trame::step(Source, receives = Go)]
fn starter(open: &mut bool) -> Status {
    match trame::recv::<Go>() {
        Some(_) => {
            *open = true;
            Status::Done
        }
        None => Status::wait().frame(),
    }
}

/// Sends jobs 0.. to `to`, after the gate opens. A full route makes it wait for room.
#[trame::step(Source)]
fn producer(open: &bool, next: &mut u32, to: &WorkerId) -> Status {
    if !*open {
        return Status::wait();
    }
    if *next == JOBS {
        return Status::Done;
    }
    match trame::send(*to, Job { id: *next }) {
        Ok(()) => {
            *next += 1;
            Status::Progress
        }
        Err(_) => Status::wait().room::<Job>(*to),
    }
}

/// Worker 1: turns each job into an event for the leader, in order. `start` is its clock at its first step.
#[trame::state(steps(consumer))]
struct Sink {
    seen: u32,
    pending: Option<u32>,
    start: Option<Reading>,
}

#[trame::step(Sink, receives = Job)]
fn consumer(seen: &mut u32, pending: &mut Option<u32>, start: &mut Option<Reading>) -> Status {
    // Worker 1's clock is the run's clock: a reading of it compares with a later one.
    let start = *start.get_or_insert_with(trame::now);
    if let Some(v) = *pending {
        return match trame::send(LEADER, Ev { v }) {
            Ok(()) => {
                *pending = None;
                Status::Progress
            }
            Err(_) => Status::wait().room::<Ev>(LEADER),
        };
    }
    if *seen == JOBS {
        // `since` aborts when the readings come from different clocks, so this also checks the clock.
        let _elapsed = trame::now().since(start);
        return Status::Done;
    }
    match trame::recv::<Job>() {
        Some(j) => {
            *seen += 1;
            *pending = Some(j.id * 10);
            Status::Progress
        }
        None => Status::wait().frame(),
    }
}

fn main() {
    let mut t = Topology::new();
    let a = t.worker();
    let b = t.worker();
    let to = b.id();
    t.set(&a, move || Source { open: false, next: 0, to });
    t.set(&b, || Sink { seen: 0, pending: None, start: None });
    t.route::<Go>(LEADER, a.id(), 1);
    t.route::<Job>(a.id(), b.id(), 2);
    t.route::<Ev>(b.id(), LEADER, 8);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    assert!(run.send(a.id(), Go { token: 1 }).is_ok(), "go did not fit");
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

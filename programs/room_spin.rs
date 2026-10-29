//! room_spin: a process `Sender` sends two items over a route of capacity 1. The second finds the route full,
//! so it reports `Blocked` and waits with `Status::wait().room`. A second process, `Ticker`, then takes
//! TICKS steps on the leader's `Go` and reports `Ticked`; only then does the leader take the first item. The
//! route has no room during those steps, so `Sender` is not called, and it sends the second item once room
//! comes. It reports how many sends failed: 1. A wait that is woken while the route is still full is called
//! during the ticks, fails again, and reports a larger count.

use trame::{Context, LEADER, Outcome, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Item {
    id: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Blocked;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Go;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Ticked;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Finished {
    failed: u32,
}

struct Sender {
    sent: u32,
    failed: u32,
    reported: bool,
}

#[trame::process]
impl Sender {
    fn step(&mut self) -> Status {
        match trame::send(LEADER, Item { id: self.sent + 1 }) {
            Ok(()) => {
                self.sent += 1;
                if self.sent == 2 {
                    assert!(trame::send(LEADER, Finished { failed: self.failed }).is_ok(), "finished did not fit");
                    return Status::Done;
                }
                Status::Progress
            }
            Err(_) => {
                self.failed += 1;
                if !self.reported {
                    self.reported = true;
                    assert!(trame::send(LEADER, Blocked).is_ok(), "blocked did not fit");
                }
                Status::wait().room::<Item>(LEADER)
            }
        }
    }
}

/// The steps `Ticker` takes while `Sender` is blocked.
const TICKS: u32 = 8;

/// Waits for `Go`, takes TICKS steps, then reports `Ticked`.
struct Ticker {
    ticks: Option<u32>,
}

#[trame::process(receives = Go)]
impl Ticker {
    fn step(&mut self) -> Status {
        match self.ticks {
            None => match trame::recv::<Go>() {
                Some(Go) => {
                    self.ticks = Some(0);
                    Status::Progress
                }
                None => Status::wait().frame(),
            },
            Some(n) if n < TICKS => {
                self.ticks = Some(n + 1);
                Status::Progress
            }
            Some(_) => {
                assert!(trame::send(LEADER, Ticked).is_ok(), "ticked did not fit");
                Status::Done
            }
        }
    }
}

fn worker(_cx: &Context) {
    let r = trame::concurrent!(Sender { sent: 0, failed: 0, reported: false }, Ticker { ticks: None };);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(worker);
    t.route::<Item>(w, LEADER, 1);
    t.route::<Blocked>(w, LEADER, 2);
    t.route::<Go>(LEADER, w, 1);
    t.route::<Ticked>(w, LEADER, 1);
    t.route::<Finished>(w, LEADER, 2);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    while run.recv::<Blocked>().is_none() {
        run.wait();
    }
    println!("sender blocked on a full route");
    assert!(run.send(w, Go).is_ok(), "go did not fit");
    while run.recv::<Ticked>().is_none() {
        run.wait();
    }
    let first = loop {
        if let Some(item) = run.recv::<Item>() {
            break item;
        }
        run.wait();
    };
    println!("item {}", first.id);
    let fin = loop {
        if let Some(f) = run.recv::<Finished>() {
            break f;
        }
        run.wait();
    };
    println!("sends failed {}", fin.failed);
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

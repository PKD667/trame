//! room_spin: a step `sender` sends two items over a route of capacity 1. The second finds the route full,
//! so it reports `Blocked` and waits with `Status::wait().room`. A second step, `ticker`, then takes
//! TICKS steps on the leader's `Go` and reports `Ticked`; only then does the leader take the first item. The
//! route has no room during those steps, and the two steps share no field, so `sender` is not called, and it
//! sends the second item once room comes. It reports how many sends failed: 1. A wait that is woken while the
//! route is still full is called during the ticks, fails again, and reports a larger count.

use trame::{LEADER, Outcome, Status, Topology};

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

/// The steps `ticker` takes while `sender` is blocked.
const TICKS: u32 = 8;

/// `sender` owns `sent`, `failed` and `reported`; `ticker` owns `ticks` (`None` until `Go`).
#[trame::state(steps(sender, ticker))]
struct Pair {
    sent: u32,
    failed: u32,
    reported: bool,
    ticks: Option<u32>,
}

#[trame::step(Pair)]
fn sender(sent: &mut u32, failed: &mut u32, reported: &mut bool) -> Status {
    match trame::send(LEADER, Item { id: *sent + 1 }) {
        Ok(()) => {
            *sent += 1;
            if *sent == 2 {
                assert!(trame::send(LEADER, Finished { failed: *failed }).is_ok(), "finished did not fit");
                return Status::Done;
            }
            Status::Progress
        }
        Err(_) => {
            *failed += 1;
            if !*reported {
                *reported = true;
                assert!(trame::send(LEADER, Blocked).is_ok(), "blocked did not fit");
            }
            Status::wait().room::<Item>(LEADER)
        }
    }
}

/// Waits for `Go`, takes TICKS steps, then reports `Ticked`.
#[trame::step(Pair, receives = Go)]
fn ticker(ticks: &mut Option<u32>) -> Status {
    match *ticks {
        None => match trame::recv::<Go>() {
            Some(Go) => {
                *ticks = Some(0);
                Status::Progress
            }
            None => Status::wait().frame(),
        },
        Some(n) if n < TICKS => {
            *ticks = Some(n + 1);
            Status::Progress
        }
        Some(_) => {
            assert!(trame::send(LEADER, Ticked).is_ok(), "ticked did not fit");
            Status::Done
        }
    }
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(Pair { sent: 0, failed: 0, reported: false, ticks: None }).id();
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

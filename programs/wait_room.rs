//! wait_room: a step sends three `Item`s to the leader over a route of capacity 1, and waits with
//! `Status::wait().room` when the route is full. The leader drains only after the step has reported that it
//! blocked, so the block is certain: the second send finds the route full. The step reports the block
//! once. The leader prints the items in order, so a wake that never comes shows as a hang, not a mismatch.

use trame::{LEADER, Outcome, Status, Topology};

const ITEMS: u32 = 3;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Item {
    id: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Blocked;

/// The state: `sent` counts the items that fit; `reported` says whether `Blocked` was sent.
#[trame::state(steps(sender))]
struct Sender {
    sent: u32,
    reported: bool,
}

/// Sends `ITEMS` items.
#[trame::step(Sender)]
fn sender(sent: &mut u32, reported: &mut bool) -> Status {
    if *sent == ITEMS {
        return Status::Done;
    }
    match trame::send(LEADER, Item { id: *sent + 1 }) {
        Ok(()) => {
            *sent += 1;
            Status::Progress
        }
        Err(_) => {
            if !*reported {
                *reported = true;
                assert!(trame::send(LEADER, Blocked).is_ok(), "blocked did not fit");
            }
            Status::wait().room::<Item>(LEADER)
        }
    }
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(Sender { sent: 0, reported: false }).id();
    t.route::<Item>(w, LEADER, 1);
    t.route::<Blocked>(w, LEADER, 2);
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
    let mut got = 0;
    while got < ITEMS {
        match run.recv::<Item>() {
            Some(item) => {
                got += 1;
                println!("item {}", item.id);
            }
            None => run.wait(),
        }
    }
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

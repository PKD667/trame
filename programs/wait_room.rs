//! wait_room: a step sends three `Item`s to the leader over a route of capacity 1, and waits with
//! `Status::wait().room` when the route is full. The leader drains only after the step has reported that it
//! blocked, so the block is certain: the second send finds the route full. The step reports the block
//! once. The leader prints the items in order, so a wake that never comes shows as a hang, not a mismatch.

use trame::{Leader, Outcome, Status, Topology};

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
    match trame::send(Leader, Item { id: *sent + 1 }) {
        Ok(()) => {
            *sent += 1;
            Status::Progress
        }
        Err(_) => {
            if !*reported {
                *reported = true;
                assert!(trame::send(Leader, Blocked).is_ok(), "blocked did not fit");
            }
            Status::wait().room::<Item>(Leader)
        }
    }
}

fn main() {
    let mut t = Topology::new();
    let w_worker = t.worker();
    t.set(&w_worker, || Sender { sent: 0, reported: false });
    let w = w_worker.id();
    t.route::<Item>(w, Leader, 1);
    t.route::<Blocked>(w, Leader, 2);
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

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

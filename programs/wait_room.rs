//! wait_room: a process sends three `Item`s to the leader over a route of capacity 1, and waits with
//! `Status::wait().room` when the route is full. The leader drains only after the process has reported that it
//! blocked, so the block is certain: the second send finds the route full. The process reports the block
//! once. The leader prints the items in order, so a wake that never comes shows as a hang, not a mismatch.

use trame::{Context, Outcome, Status, Topology, LEADER};

const ITEMS: u32 = 3;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Item {
    id: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Blocked;

/// Sends `ITEMS` items; `sent` counts those that fit.
struct Sender {
    sent: u32,
    reported: bool,
}

#[trame::process]
impl Sender {
    fn step(&mut self) -> Status {
        if self.sent == ITEMS {
            return Status::Done;
        }
        match trame::send(LEADER, Item { id: self.sent + 1 }) {
            Ok(()) => {
                self.sent += 1;
                Status::Progress
            }
            Err(_) => {
                if !self.reported {
                    self.reported = true;
                    assert!(trame::send(LEADER, Blocked).is_ok(), "blocked did not fit");
                }
                Status::wait().room::<Item>(LEADER)
            }
        }
    }
}

fn worker(_cx: &Context) {
    let r = trame::concurrent!(Sender { sent: 0, reported: false };);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(worker);
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

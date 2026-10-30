//! pipeline_chain: six workers in a chain, routes of capacity 4. Worker 0 generates items 0..4000, and
//! every worker maps x to x*MUL + s + 1 (wrapping, s its position) and passes it on; the last sends to the
//! leader. The leader asserts the items arrive in order and prints the count, the first and last values
//! and an xor and a wrapping sum of all of them. Every value crosses five worker-to-worker links, so a
//! lost, repeated or reordered item changes the output. Expected output: pipeline_chain.expected, from an
//! independent recomputation in Python.

use trame::{Context, LEADER, Outcome, Status, Topology, WorkerId};

const WORKERS: usize = 6;
const ITEMS: u64 = 4000;
const MUL: u64 = 6364136223846793005;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Item {
    seq: u64,
    x: u64,
}

/// Generates the items. `pending` is an item made and not yet accepted by the route.
struct Source {
    next: u64,
    to: WorkerId,
    pending: Option<Item>,
}

#[trame::process]
impl Source {
    fn step(&mut self) -> Status {
        if let Some(it) = self.pending.take() {
            if trame::send(self.to, Item { seq: it.seq, x: it.x }).is_err() {
                self.pending = Some(it);
                return Status::wait().room::<Item>(self.to);
            }
        }
        if self.next == ITEMS {
            return Status::Done;
        }
        let x = self.next.wrapping_mul(MUL).wrapping_add(1);
        self.pending = Some(Item { seq: self.next, x });
        self.next += 1;
        Status::Progress
    }
}

struct Stage {
    s: u64,
    to: WorkerId,
    seen: u64,
    pending: Option<Item>,
}

#[trame::process(receives = Item)]
impl Stage {
    fn step(&mut self) -> Status {
        if let Some(it) = self.pending.take() {
            if trame::send(self.to, Item { seq: it.seq, x: it.x }).is_err() {
                self.pending = Some(it);
                return Status::wait().room::<Item>(self.to);
            }
        }
        if self.seen == ITEMS {
            return Status::Done;
        }
        match trame::recv::<Item>() {
            Some(it) => {
                assert_eq!(it.seq, self.seen, "an item out of order, lost or duplicated");
                self.seen += 1;
                self.pending = Some(Item { seq: it.seq, x: it.x.wrapping_mul(MUL).wrapping_add(self.s + 1) });
                Status::Progress
            }
            None => Status::wait().frame(),
        }
    }
}

fn main() {
    let mut t = Topology::new();
    let ws: Vec<WorkerId> = (0..WORKERS)
        .map(|s| {
            t.worker(move |cx: &Context| {
                let ws = cx.workers();
                let to = if s + 1 < WORKERS { ws[s + 1] } else { LEADER };
                if s == 0 {
                    assert_eq!(trame::concurrent!(Source { next: 0, to, pending: None };), Ok(()));
                } else {
                    assert_eq!(trame::concurrent!(Stage { s: s as u64, to, seen: 0, pending: None };), Ok(()));
                }
            })
        })
        .collect();
    for s in 0..WORKERS {
        t.route::<Item>(ws[s], if s + 1 < WORKERS { ws[s + 1] } else { LEADER }, 4);
    }
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let (mut n, mut xor, mut sum, mut first, mut last) = (0u64, 0u64, 0u64, 0u64, 0u64);
    while n < ITEMS {
        match run.recv::<Item>() {
            Some(it) => {
                assert_eq!(it.seq, n, "the leader took an item out of order");
                if n == 0 {
                    first = it.x;
                }
                last = it.x;
                xor ^= it.x;
                sum = sum.wrapping_add(it.x);
                n += 1;
            }
            None => run.wait(),
        }
    }
    println!("items {n}");
    println!("first {first}");
    println!("last {last}");
    println!("xor {xor}");
    println!("sum {sum}");
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

//! pipeline_chain: six workers in a chain, routes of capacity 4. Worker 0 generates items 0..4000, and
//! every worker maps x to x*MUL + s + 1 (wrapping, s its position) and passes it on; the last sends to the
//! leader. The leader asserts the items arrive in order and prints the count, the first and last values
//! and an xor and a wrapping sum of all of them. Every value crosses five worker-to-worker links, so a
//! lost, repeated or reordered item changes the output. Expected output: pipeline_chain.expected, from an
//! independent recomputation in Python.

use trame::{Leader, Outcome, Status, Topology, WorkerId};

const WORKERS: usize = 6;
const ITEMS: u64 = 4000;
const MUL: u64 = 6364136223846793005;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Item {
    seq: u64,
    x: u64,
}

/// Worker 0: the next item to make, and an item made and not yet accepted by the route.
#[trame::state(steps(source))]
struct Source {
    next: u64,
    to: WorkerId,
    pending: Option<Item>,
}

#[trame::step(Source)]
fn source(next: &mut u64, to: &WorkerId, pending: &mut Option<Item>) -> Status {
    if let Some(it) = pending.take() {
        if trame::send(*to, it).is_err() {
            *pending = Some(it);
            return Status::wait().room::<Item>(*to);
        }
    }
    if *next == ITEMS {
        return Status::Done;
    }
    *pending = Some(Item { seq: *next, x: next.wrapping_mul(MUL).wrapping_add(1) });
    *next += 1;
    Status::Progress
}

/// A stage at position `s` of the chain: the items seen, and one mapped item not yet passed on. The
/// last stage passes to the leader, the others to the next worker; the two are separate state types,
/// since a route's end is a worker or the leader, and a stage knows which when it is declared.
#[trame::state(steps(stage))]
struct Stage {
    s: u64,
    to: WorkerId,
    seen: u64,
    pending: Option<Item>,
}

#[trame::state(steps(last))]
struct Last {
    s: u64,
    seen: u64,
    pending: Option<Item>,
}

/// Send the pending item to `to`, or take the next item and map it. A macro, since `to` is a worker
/// in one step and the leader in the other.
macro_rules! pass {
    ($s:expr, $to:expr, $seen:expr, $pending:expr) => {{
        if let Some(it) = $pending.take() {
            if trame::send($to, it).is_err() {
                *$pending = Some(it);
                return Status::wait().room::<Item>($to);
            }
        }
        if *$seen == ITEMS {
            return Status::Done;
        }
        match trame::recv::<Item>() {
            Some(it) => {
                assert_eq!(it.seq, *$seen, "an item out of order, lost or duplicated");
                *$seen += 1;
                *$pending = Some(Item { seq: it.seq, x: it.x.wrapping_mul(MUL).wrapping_add($s + 1) });
                Status::Progress
            }
            None => Status::wait().frame(),
        }
    }};
}

#[trame::step(Stage, receives = Item)]
fn stage(s: &u64, to: &WorkerId, seen: &mut u64, pending: &mut Option<Item>) -> Status {
    pass!(*s, *to, seen, pending)
}

#[trame::step(Last, receives = Item)]
fn last(s: &u64, seen: &mut u64, pending: &mut Option<Item>) -> Status {
    pass!(*s, Leader, seen, pending)
}

fn main() {
    let mut t = Topology::new();
    let src = t.worker::<Source>();
    let stages: Vec<_> = (1..WORKERS - 1).map(|_| t.worker::<Stage>()).collect();
    let end = t.worker::<Last>();
    let mut ids = vec![src.id()];
    ids.extend(stages.iter().map(|w| w.id()));
    ids.push(end.id());
    t.set(&src, {
        let to = ids[1];
        move || Source { next: 0, to, pending: None }
    });
    for (k, w) in stages.iter().enumerate() {
        let (s, to) = (k as u64 + 1, ids[k + 2]);
        t.set(w, move || Stage { s, to, seen: 0, pending: None });
    }
    t.set(&end, || Last { s: WORKERS as u64 - 1, seen: 0, pending: None });
    for i in 0..WORKERS - 1 {
        t.route::<Item>(ids[i], ids[i + 1], 4);
    }
    t.route::<Item>(end.id(), Leader, 4);
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

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

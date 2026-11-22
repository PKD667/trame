//! xhost_ring: routes between workers of different hosts (CONTRACT.md F3, F5). Each host runs three
//! workers. Worker k of host h sends 2000 numbered items over a lossless route of capacity 4 to worker
//! k+1 (mod 3) of host h+1 (mod H), and a tick per item over a lossy route of capacity 1 beside it. On
//! one host, the routes stay on it. A receiver asserts that items arrive in order and exactly once, and
//! that the ticks it sees increase; it reports the count and sum of its items to its leader, which
//! prints them. Item `seq` of the worker numbered g = 3h + k carries seq*(g+1)+7, so the sum received
//! from g is (g+1)*M(M-1)/2 + 7M. Expected output: xhost_ring.expected (one host) and
//! xhost_ring.h2.expected (two), from that formula.

use trame::{HostId, Leader, Outcome, Status, Topology, WorkerId};

const WORKERS: u32 = 3;
const ITEMS: u64 = 2000;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Item {
    seq: u64,
    val: u64,
}

#[trame::message]
struct Tick {
    seq: u64,
}

#[trame::message]
struct Report {
    k: u64,
    from: u64,
    count: u64,
    sum: u64,
}

/// One worker: where it sends, what it has sent and received, and the last tick it saw.
#[trame::state(steps(ring))]
struct Node {
    k: u32,
    g: u64,
    from: u64,
    to: WorkerId,
    sent: u64,
    got: u64,
    sum: u64,
    tick: Option<u64>,
    reported: bool,
}

/// One call sends what the route takes and receives what has arrived, then waits for either.
#[trame::step(Node, receives = Item, Tick)]
#[allow(clippy::too_many_arguments)]
fn ring(
    k: &u32,
    g: &u64,
    from: &u64,
    to: &WorkerId,
    sent: &mut u64,
    got: &mut u64,
    sum: &mut u64,
    tick: &mut Option<u64>,
    reported: &mut bool,
) -> Status {
    let mut moved = false;
    while *sent < ITEMS {
        let seq = *sent;
        if trame::send(*to, Item { seq, val: seq * (*g + 1) + 7 }).is_err() {
            break;
        }
        trame::send(*to, Tick { seq }).expect("a lossy send never fails");
        *sent += 1;
        moved = true;
    }
    while let Some(it) = trame::recv::<Item>() {
        assert_eq!(it.seq, *got, "an item out of order, lost or duplicated");
        *got += 1;
        *sum += it.val;
        moved = true;
    }
    while let Some(t) = trame::recv::<Tick>() {
        assert!(tick.is_none_or(|p| t.seq > p), "a lossy route delivered an older tick after a newer");
        *tick = Some(t.seq);
    }
    if *sent == ITEMS && *got == ITEMS {
        if !*reported {
            if trame::send(Leader, Report { k: u64::from(*k), from: *from, count: *got, sum: *sum }).is_err() {
                return Status::wait().room::<Report>(Leader);
            }
            *reported = true;
        }
        return Status::Done;
    }
    if moved {
        return Status::Progress;
    }
    let w = Status::wait().frame();
    if *sent < ITEMS { w.room::<Item>(*to) } else { w }
}

fn main() {
    let (me, hosts) = (trame::host().0, trame::hosts());
    let (next, prev) = ((me + 1) % hosts, (me + hosts - 1) % hosts);
    let at = |h: u32, k: u32| WorkerId::from((HostId(h), k % WORKERS));
    let mut t = Topology::new();
    let ws: Vec<_> = (0..WORKERS).map(|_| t.worker::<Node>()).collect();
    for (k, w) in ws.iter().enumerate() {
        let k = k as u32;
        let (to, src) = (at(next, k + 1), at(prev, k + WORKERS - 1));
        let from = u64::from(prev * WORKERS + (k + WORKERS - 1) % WORKERS);
        let g = u64::from(me * WORKERS + k);
        t.set(w, move || Node { k, g, from, to, sent: 0, got: 0, sum: 0, tick: None, reported: false });
        t.route::<Item>(w.id(), to, 4);
        t.lossy::<Tick>(w.id(), to, 1);
        // With more than one host, the route into this worker comes from the previous host, which
        // declares it too (F3).
        if hosts > 1 {
            t.route::<Item>(src, w.id(), 4);
            t.lossy::<Tick>(src, w.id(), 1);
        }
        t.route::<Report>(w.id(), Leader, 1);
    }
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e}");
            return;
        }
    };
    let mut lines = vec![None; WORKERS as usize];
    let mut n = 0;
    while n < WORKERS {
        match run.recv::<Report>().map(|s| s.value) {
            Some(r) => {
                lines[r.k as usize] =
                    Some(format!("host {me} worker {} from {} count {} sum {}", r.k, r.from, r.count, r.sum));
                n += 1;
            }
            None => run.wait(),
        }
    }
    for l in lines {
        println!("{}", l.unwrap());
    }
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

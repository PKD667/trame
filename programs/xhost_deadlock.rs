//! xhost_deadlock: a cycle of full routes between two hosts (spec.md 5, Deadlock; CONTRACT.md F5).
//! Each of two workers sends ten items to the other over a lossless route of capacity 4 before it
//! receives any. Both fill their route and wait for room, which only the other could make: neither
//! step can run again. Each host has two workers. On two hosts the cycle joins worker 0 of host 0 and
//! worker 0 of host 1, its frames and credits cross hosts, and worker 1 of each returns at once; the
//! leaders' detector shows the cycle, and each host prints `host h: [Deadlock, Returned]`
//! (xhost_deadlock.h2.expected). On one host the two are workers 0 and 1, and the
//! leader prints `host 0: [Deadlock, Deadlock]`, as on seq (xhost_deadlock.expected).

use trame::{HostId, Status, Topology, WorkerId};

#[trame::message]
struct Item {
    n: u64,
}

/// One end of the cycle: where it sends, and how many items it has sent and taken.
#[trame::state(steps(swap))]
struct End {
    to: WorkerId,
    sent: u64,
    got: u64,
}

#[trame::step(End, receives = Item)]
fn swap(to: &WorkerId, sent: &mut u64, got: &mut u64) -> Status {
    while *sent < 10 {
        if trame::send(*to, Item { n: *sent }).is_err() {
            return Status::wait().room::<Item>(*to);
        }
        *sent += 1;
    }
    while trame::recv::<Item>().is_some() {
        *got += 1;
    }
    if *got == 10 { Status::Done } else { Status::wait().frame() }
}

/// A worker with nothing to do.
#[trame::state(steps(quick))]
struct Quick {}

#[trame::step(Quick)]
fn quick() -> Status {
    Status::Done
}

fn main() {
    let (me, hosts) = (trame::host().0, trame::hosts());
    let mut t = Topology::new();
    let at = |h: u32, k: u32| WorkerId::from((HostId(h), k));
    let pairs = if hosts > 1 {
        let w = t.worker::<End>();
        let other = at(1 - me, 0);
        t.set(&w, move || End { to: other, sent: 0, got: 0 });
        let q = t.worker::<Quick>();
        t.set(&q, || Quick {});
        vec![(at(0, 0), at(1, 0)), (at(1, 0), at(0, 0))]
    } else {
        let a = t.worker::<End>();
        let b = t.worker::<End>();
        let (ia, ib) = (a.id(), b.id());
        t.set(&a, move || End { to: ib, sent: 0, got: 0 });
        t.set(&b, move || End { to: ia, sent: 0, got: 0 });
        vec![(ia, ib), (ib, ia)]
    };
    for (from, to) in pairs {
        t.route::<Item>(from, to, 4);
    }
    let run = t.launch().expect("the launch is accepted");
    println!("host {me}: {:?}", run.join());
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

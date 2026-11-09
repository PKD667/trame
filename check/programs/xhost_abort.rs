//! xhost_abort: an abort on one host while a worker of another host waits on a route from it (CONTRACT.md
//! E1, F5). The sender sends three items, then panics; the receiver takes the items and waits for a
//! fourth. Each host has two workers. On two hosts the sender is worker 0 of host 1 and the receiver
//! worker 1 of host 0, and the other two return at once: host 1 aborts, and its leader panics with
//! `trame: worker 0 panicked: the sender fails on purpose` (xhost_abort.panics); host 0 runs on, and its
//! receiver waits for a frame no one can send, which the detector shows: it prints
//! `host 0: [Returned, Deadlock]` (xhost_abort.h2.expected). Neither host hangs. On one host the two are
//! workers 0 and 1 of it, and the leader panics with the same line, printing nothing
//! (xhost_abort.expected).

use trame::{HostId, Status, Topology, WorkerId};

#[trame::message]
struct Item {
    n: u64,
}

/// The sender: how many items it has sent.
#[trame::state(steps(send))]
struct Sender {
    to: WorkerId,
    sent: u64,
}

#[trame::step(Sender)]
fn send(to: &WorkerId, sent: &mut u64) -> Status {
    while *sent < 3 {
        if trame::send(*to, Item { n: *sent }).is_err() {
            return Status::wait().room::<Item>(*to);
        }
        *sent += 1;
    }
    panic!("the sender fails on purpose");
}

/// The receiver: how many items it has taken. It asserts they come in order.
#[trame::state(steps(take))]
struct Receiver {
    got: u64,
}

#[trame::step(Receiver, receives = Item)]
fn take(got: &mut u64) -> Status {
    while let Some(it) = trame::recv::<Item>() {
        assert_eq!(it.n, *got, "an item out of order");
        *got += 1;
    }
    Status::wait().frame()
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
    let (from, to) = if hosts > 1 {
        let (s, r) = (WorkerId::from((HostId(1), 0)), WorkerId::from((HostId(0), 1)));
        if me == 0 {
            let q = t.worker::<Quick>();
            t.set(&q, || Quick {});
            let w = t.worker::<Receiver>();
            t.set(&w, || Receiver { got: 0 });
        } else {
            let w = t.worker::<Sender>();
            t.set(&w, move || Sender { to: r, sent: 0 });
            let q = t.worker::<Quick>();
            t.set(&q, || Quick {});
        }
        (s, r)
    } else {
        let s = t.worker::<Sender>();
        let r = t.worker::<Receiver>();
        let rid = r.id();
        t.set(&s, move || Sender { to: rid, sent: 0 });
        t.set(&r, || Receiver { got: 0 });
        (s.id(), rid)
    };
    t.route::<Item>(from, to, 4);
    let run = t.launch().expect("the launch is accepted");
    println!("host {me}: {:?}", run.join());
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

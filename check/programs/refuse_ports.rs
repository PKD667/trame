//! refuse_ports: two hosts that disagree on a route between them are refused (CONTRACT.md F3). Host
//! 0 declares the route of `Item` from its worker 0 to worker 0 of host 1 with capacity 4, and host 1
//! declares it with capacity 8. Each host prints its refusal. On one host there is no other host, and
//! the program runs its one worker. Expected output: refuse_ports.expected (one host) and
//! refuse_ports.h2.expected (two).

use trame::{HostId, Status, Topology, WorkerId};

#[trame::message]
struct Item {
    x: u64,
}

#[trame::state(steps(idle))]
struct Idle {
    n: u64,
}

#[trame::step(Idle)]
fn idle(n: &mut u64) -> Status {
    *n += 1;
    Status::Done
}

fn main() {
    let (me, hosts) = (trame::host().0, trame::hosts());
    let mut t = Topology::new();
    let w = t.worker::<Idle>();
    t.set(&w, || Idle { n: 0 });
    if hosts > 1 {
        let far = |h: u32| WorkerId::from((HostId(h), 0));
        match me {
            0 => t.route::<Item>(w.id(), far(1), 4),
            _ => t.route::<Item>(far(0), w.id(), 8),
        }
    }
    match t.launch() {
        Ok(run) => println!("host {me}: {:?}", run.join()),
        Err(e) => println!("host {me} refused: {e}"),
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

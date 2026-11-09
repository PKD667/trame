//! route_one_side: a route between hosts that only one host declares is refused there, and the other
//! host runs (CONTRACT.md F3, F7). Host 0 declares the route of `Item` from its worker 0 to worker 0 of
//! host 1; host 1 declares no route. Host 0 waits for host 1's answer, which comes at the latest when
//! host 1 ends, and is refused; host 1 never waits for host 0. On one host there is no other host,
//! and the program runs its one worker. Expected output: route_one_side.expected (one host) and
//! route_one_side.h2.expected (two).

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
    if hosts > 1 && me == 0 {
        t.route::<Item>(w.id(), WorkerId::from((HostId(1), 0)), 4);
    }
    match t.launch() {
        Ok(run) => println!("host {me}: {:?}", run.join()),
        Err(e) => println!("host {me} refused: {e}"),
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

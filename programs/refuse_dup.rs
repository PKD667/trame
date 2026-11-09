//! refuse_dup: two routes with the same sender, receiver and tag. The launch checks refuse the topology
//! before any worker starts (INTERFACE.md 4, Launch checks). Under MPI each rank refuses the same way.

use trame::{Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Dup {
    n: u32,
}

/// Never runs: the launch is refused.
#[trame::state(steps(idle))]
struct Idle {}

#[trame::step(Idle)]
fn idle() -> Status {
    Status::Done
}

fn main() {
    let mut t = Topology::new();
    let a = t.worker();
    t.set(&a, move || Idle {});
    let b = t.worker();
    t.set(&b, move || Idle {});
    t.route::<Dup>(a.id(), b.id(), 1);
    t.route::<Dup>(a.id(), b.id(), 1);
    match t.launch() {
        Ok(_) => println!("launched"),
        Err(e) => println!("refused: {e:?}"),
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

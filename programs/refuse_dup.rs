//! refuse_dup: two routes with the same sender, receiver and tag. The launch checks refuse the topology
//! before any worker starts (spec.md 4, Launch checks). Under MPI each rank refuses the same way.

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
    let a = t.worker(Idle {});
    let b = t.worker(Idle {});
    t.route::<Dup>(a.id(), b.id(), 1);
    t.route::<Dup>(a.id(), b.id(), 1);
    match t.launch() {
        Ok(_) => println!("launched"),
        Err(e) => println!("refused: {e:?}"),
    }
}

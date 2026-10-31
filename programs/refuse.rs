//! refuse: a route of capacity 0 fails the launch checks (spec.md 4). Every rank refuses the same way,
//! and no worker starts. Under MPI each rank prints the same line, so run.sh de-duplicates this output.

use trame::{Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Never {
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
    t.route::<Never>(a.id(), b.id(), 0);
    match t.launch() {
        Ok(_) => println!("launched"),
        Err(e) => println!("refused: {e:?}"),
    }
}

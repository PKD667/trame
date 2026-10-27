//! refuse: a route of capacity 0 fails the launch checks (spec.md 4). Every rank refuses the same way,
//! and no worker starts. Under MPI each rank prints the same line, so run.sh de-duplicates this output.

use trame::{Context, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Never {
    n: u32,
}

/// Never runs: the launch is refused.
fn idle(_cx: &Context) {}

fn main() {
    let mut t = Topology::new();
    let a = t.worker(idle);
    let b = t.worker(idle);
    t.route::<Never>(a, b, 0);
    match t.launch() {
        Ok(_) => println!("launched"),
        Err(e) => println!("refused: {e:?}"),
    }
}

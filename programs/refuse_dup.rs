//! refuse_dup: two routes with the same sender, receiver and tag. The launch checks refuse the topology
//! before any worker starts (spec.md 4, Launch checks). Under MPI each rank refuses the same way.

use trame::{Context, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Dup {
    n: u32,
}

/// Never runs: the launch is refused.
fn idle(_cx: &Context) {}

fn main() {
    let mut t = Topology::new();
    let a = t.worker(idle);
    let b = t.worker(idle);
    t.route::<Dup>(a, b, 1);
    t.route::<Dup>(a, b, 1);
    match t.launch() {
        Ok(_) => println!("launched"),
        Err(e) => println!("refused: {e:?}"),
    }
}

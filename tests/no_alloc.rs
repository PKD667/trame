//! A program that does not install `trame::Alloc` is refused at launch, on every backend (domain.rs).
#![cfg(trame_checked)]

use trame::{Refused, Status, Topology};

#[trame::state(steps(idle))]
struct Idle {
    n: u32,
}

#[trame::step(Idle)]
fn idle(n: &mut u32) -> Status {
    *n += 1;
    Status::Done
}

#[test]
fn a_launch_without_trame_alloc_is_refused() {
    let mut t = Topology::new();
    let w = t.worker();
    t.set(&w, || Idle { n: 0 });
    assert!(matches!(t.launch(), Err(Refused::Alloc)));
}

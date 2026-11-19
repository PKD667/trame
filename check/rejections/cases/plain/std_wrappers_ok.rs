//@check-pass
// CONTRACT.md, M2: the std wrappers that add no pointer are plain when what they hold is: `Reverse` (a min-heap is
// `BoundedHeap<Reverse<T>>`), `Wrapping` and `Saturating`.
use core::cmp::Reverse;
use core::num::{Saturating, Wrapping};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, trame::Plain)]
struct Due(u64);

#[trame::state(steps(step))]
struct Hold {
    soonest: trame::BoundedHeap<Reverse<Due>>,
    wrap: Wrapping<u32>,
    sat: Saturating<u8>,
}

#[trame::step(Hold)]
fn step(soonest: &mut trame::BoundedHeap<Reverse<Due>>, wrap: &mut Wrapping<u32>, sat: &mut Saturating<u8>) -> trame::Status {
    let _ = soonest.push(Reverse(Due(3)));
    *wrap += Wrapping(1);
    *sat += Saturating(1);
    trame::Status::Done
}

fn main() {}

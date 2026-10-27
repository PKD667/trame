//! expect: refused [deny] worker
// spec: Requirement (All levels): no UnsafeCell, so no interior mutability. A worker writes through a Cell.
use core::cell::Cell;

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let c = Cell::new(1u32);
        c.set(2);
    });
}

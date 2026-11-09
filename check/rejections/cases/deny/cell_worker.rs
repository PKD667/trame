// spec: Requirement (All levels): no UnsafeCell, so no interior mutability. A worker writes through a Cell.
use core::cell::Cell;

fn main() {
    trame::__check::worker(&|| {
        let c = Cell::new(1u32); //~ ERROR: [deny] worker
        c.set(2); //~ ERROR: [deny] worker
    });
}

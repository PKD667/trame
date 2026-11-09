// spec: Requirement (All levels): no UnsafeCell, so no interior mutability. A worker uses an atomic, which core builds on an
// UnsafeCell. The refusal is the deny list's: core::sync::* (crates/driver/lists/deny.txt; A_W.txt).
use core::sync::atomic::{AtomicU32, Ordering};

fn main() {
    trame::__check::worker(&|| {
        let a = AtomicU32::new(1); //~ ERROR: [deny] worker
        a.fetch_add(1, Ordering::SeqCst); //~ ERROR: [deny] worker
    });
}

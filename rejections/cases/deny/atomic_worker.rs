//! expect: refused [deny] worker
// spec: Requirement (All levels): no shared memory outside the cells and the effects. A worker uses an atomic.
use core::sync::atomic::{AtomicU32, Ordering};

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let a = AtomicU32::new(1);
        a.fetch_add(1, Ordering::SeqCst);
    });
}

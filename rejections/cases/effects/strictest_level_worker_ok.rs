//! expect: accepted
// spec: Section 3 (driver/README.md). This is the pair of strictest_level_item.rs: helper, called only from a
// worker, is checked at the worker's level, where barrier is allowed. Accepted.
fn helper() {
    let _ = trame::barrier();
}

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| helper());
}

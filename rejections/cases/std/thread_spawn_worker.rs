//! expect: refused [std] worker, [unsafe] worker
// spec: Requirement (All levels): no threads, and no std. A worker spawns a thread.
fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = std::thread::spawn(|| {});
    });
}

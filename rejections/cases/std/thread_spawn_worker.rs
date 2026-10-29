// spec: Requirement (All levels): no threads, and no std. A worker spawns a thread.
//@error-in-other-file: [std] worker
fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = std::thread::spawn(|| {}); //~ ERROR: [std] worker
    });
}

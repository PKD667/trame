// spec: Requirement (All levels): no unsafe, at the worker level too. A worker reads through a raw pointer.
fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let x = 1u32;
        let p = &x as *const u32;
        let _ = unsafe { *p }; //~ ERROR: [unsafe] worker
    });
}

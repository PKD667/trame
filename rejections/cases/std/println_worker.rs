//! expect: refused [std] worker
// spec: Requirement (Worker): a worker reaches the outside world only through effects, so it never calls the OS.
fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        std::println!("hello");
    });
}

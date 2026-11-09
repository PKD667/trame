// spec: Requirement (All levels): no threads, and no std. A worker spawns a thread.
//@error-in-other-file: [std] worker
fn main() {
    trame::__check::worker(&|| {
        let _ = std::thread::spawn(|| {}); //~ ERROR: [std] worker
    });
}

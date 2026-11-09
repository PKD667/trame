// spec: Requirement (All levels): no unsafe, at the worker level too. A worker reads through a raw pointer.
fn main() {
    trame::__check::worker(&|| {
        let x = 1u32;
        let p = &x as *const u32;
        let _ = unsafe { *p }; //~ ERROR: [unsafe] worker
    });
}

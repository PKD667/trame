//@check-pass
// spec: Requirement (All levels): no UnsafeCell. This is the pair of cell_worker.rs: the same write is a
// plain `mut` local, which needs no interior mutability, and is accepted.
fn main() {
    trame::__check::worker(&|| {
        let mut c = 1u32;
        c = 2;
        let _ = c;
    });
}

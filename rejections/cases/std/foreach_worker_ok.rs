//@check-pass
// spec: Requirement (Worker): a worker calls items of A_W. This is the pair of foreach_worker.rs: the closure
// passed to for_each does arithmetic only, and is accepted.
fn main() {
    trame::__check::worker(&|| {
        let mut sum = 0u32;
        [1u32, 2].iter().for_each(|x| sum += *x);
        let _ = sum;
    });
}

//! expect: accepted
// spec: Definition (Effect sets): Sigma_W contains barrier. This is the pair of barrier_step.rs: the same
// barrier, in a worker.
fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = trame::barrier();
    });
}

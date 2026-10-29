//@check-pass
// spec: Definition (Effect sets): Sigma_W includes barrier. driver/README.md:21 gives the strictest-level rule. This is the pair
// of strictest_level_item.rs: helper, called only from a worker, is checked at the worker's level, where barrier is allowed.
fn helper() {
    let _ = trame::barrier();
}

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| helper());
}

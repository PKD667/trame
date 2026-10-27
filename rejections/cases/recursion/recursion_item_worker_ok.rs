//! expect: accepted
// spec: Requirement (Worker): a worker may recurse. The worker level of recursion_item.rs: the same recursive
// function, reached from a worker, is accepted.
fn down(n: u32) -> u32 {
    if n == 0 { 0 } else { down(n - 1) }
}

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = down(3);
    });
}

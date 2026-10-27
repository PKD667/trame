//! expect: accepted
// spec: Requirement (Worker): a worker may allocate. The worker level of the allocation pair: the push that an
// item may not do (vec_push_item.rs) is accepted here.
fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let mut v: Vec<u32> = Vec::new();
        v.push(1);
        let _ = v.len();
    });
}

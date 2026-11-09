//@check-pass
// spec: Requirement (Worker): a worker may allocate. The worker level of the allocation pair: the push that an
// item may not do (vec_push_item.rs) is accepted here.
fn main() {
    trame::__check::worker(&|| {
        let mut v: Vec<u32> = Vec::new();
        v.push(1);
        let _ = v.len();
    });
}

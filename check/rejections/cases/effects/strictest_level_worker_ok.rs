//@check-pass
// spec: Requirement (Worker): a worker may allocate. crates/driver/README.md gives the strictest-level rule. This is the pair
// of strictest_level_item.rs: helper, called only from a worker, is checked at the worker's level, where Vec::push is allowed.
fn helper() {
    let mut v: Vec<u32> = Vec::new();
    v.push(1);
}

fn main() {
    trame::__check::worker(&|| helper());
}

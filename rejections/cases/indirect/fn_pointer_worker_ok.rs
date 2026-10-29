//@check-pass
// spec: Requirement (All levels) forbids indirect calls. This is the pair of fn_pointer_worker.rs: the
// parameter is a generic `impl Fn`, so the function item `id` is called directly after monomorphization.
fn apply(f: impl Fn(u32) -> u32, x: u32) -> u32 {
    f(x)
}

fn id(x: u32) -> u32 {
    x
}

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = apply(id, 3);
    });
}

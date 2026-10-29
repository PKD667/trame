// spec: Requirement (All levels): no indirect calls. A worker calls through a trait object, &dyn Fn.
fn apply(f: &dyn Fn(u32) -> u32, x: u32) -> u32 {
    f(x) //~ ERROR: [indirect] worker
}

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = apply(&|x| x + 1, 3);
    });
}

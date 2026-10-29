// spec: Requirement (All levels): no indirect calls. A worker calls a function pointer, fn(u32) -> u32.
fn apply(f: fn(u32) -> u32, x: u32) -> u32 {
    f(x) //~ ERROR: [indirect] worker
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

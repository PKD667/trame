// spec: Requirement (All levels): no globals, at any level. A worker reads a static.
static COUNT: u32 = 7;

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = COUNT + 1; //~ ERROR: [static] worker
    });
}

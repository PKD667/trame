//! expect: refused [allocation] item, [effects] item
// spec: Section 3 (driver/README.md): a function is checked at the strictest level that reaches it. helper calls
// barrier, which a worker may use but an item may not. The worker use is legal, the item use is not, so the
// helper is refused. This is the over-refusal boundary: the matched pair is strictest_level_worker_ok.rs.
fn helper() {
    let _ = trame::barrier();
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn bump(x: &Work, slot: &mut u32, _cx: &()) {
    helper();
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(bump, &(), &items, &mut slots);
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| helper());
}

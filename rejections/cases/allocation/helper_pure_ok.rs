//@check-pass
// spec: Requirement (Item). This is the pair of helper_alloc_from_item.rs: the helper does arithmetic only, so
// it is legal at the item level and at the worker level, and is accepted at both.
fn remember(n: u32) -> u32 {
    n + 1
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn bump(x: &Work, slot: &mut u32, _cx: &()) {
    *slot = remember(x.by);
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(bump, &(), &items, &mut slots);
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = remember(1);
    });
}

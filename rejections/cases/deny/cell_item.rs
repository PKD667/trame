//! expect: refused [deny] item
// spec: Requirement (All levels): no UnsafeCell, so no interior mutability. An item writes through a Cell.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn bump(x: &Work, slot: &mut u32, _cx: &()) {
    let c = core::cell::Cell::new(x.by);
    c.set(c.get() + 1);
    *slot = c.get();
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(bump, &(), &items, &mut slots);
}

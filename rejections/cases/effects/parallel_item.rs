//! expect: refused [effects] item
// spec: Requirement (Item), Definition (Effect sets): Sigma_I is empty. An item may not run parallel!: that is an
// effect, and an item is what parallel! runs, not what runs it.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn bump(x: &Work, slot: &mut u32, _cx: &()) {
    *slot += x.by;
}

#[trame::item(key = x.index as usize)]
fn outer(x: &Work, slot: &mut u32, _cx: &()) {
    let items = [Work { index: 0, by: x.by }];
    let mut slots = [0_u32];
    trame::parallel!(bump, &(), &items, &mut slots);
    *slot = slots[0];
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(outer, &(), &items, &mut slots);
}

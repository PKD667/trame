//! expect: refused [effects] item
// spec: Requirement (Item), Definition (Effect sets): Sigma_I is empty. An item reads the clock, which is an effect.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn stamp(x: &Work, slot: &mut u32, _cx: &()) {
    let _ = trame::now();
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(stamp, &(), &items, &mut slots);
}

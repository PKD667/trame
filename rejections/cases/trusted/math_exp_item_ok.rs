//@check-pass
// spec: Section 7 (math::f32::exp): a numeric function trame provides is pure code at the item level, not an effect.
// driver/README.md: a function defined in trame is trusted at every level, so its body is not read.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn grow(x: &Work, slot: &mut u32, _cx: &()) {
    *slot = trame::math::f32::exp(x.by as f32) as u32;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(grow, &(), &items, &mut slots);
}

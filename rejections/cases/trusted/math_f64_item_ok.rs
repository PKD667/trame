//@check-pass
// spec: Section 7 (math::f64): the portable f64 functions are pure code at the item level, not effects.
// driver/README.md: a function defined in trame is trusted at every level, so its body is not read.
struct Work {
    index: u32,
    by: f64,
}

#[trame::item(key = x.index as usize)]
fn grow(x: &Work, slot: &mut f64, _cx: &()) {
    let v = x.by;
    let a = if v < 0.0 { -v } else { v };
    *slot = trame::math::f64::exp(v)
        + trame::math::f64::ln(a + 1.0)
        + trame::math::f64::sqrt(a)
        + trame::math::f64::round(v)
        + trame::math::f64::powi(v, 3)
        + trame::math::f64::powf(a + 1.0, 0.5)
        + trame::math::f64::exp_m1(v);
}

fn main() {
    let items = [Work { index: 0, by: 1.5 }];
    let mut slots = [0.0_f64];
    trame::parallel!(grow, &(), &items, &mut slots);
}

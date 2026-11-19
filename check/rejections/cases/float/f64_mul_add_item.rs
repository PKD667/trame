// spec: Requirement (Floating point): no FMA contraction. f64::mul_add is fused on hardware with FMA and a libm call
// elsewhere, so an item may not call it; trame::math::f64::mul_add is the same bits everywhere.
struct W {
    x: f64,
}

#[trame::item(key = 0)]
fn fused(w: &W, slot: &mut f64, _cx: &()) {
    *slot = w.x.mul_add(2.0, 1.0); //~ ERROR: [std] item
}

fn main() {
    let items = [W { x: 1.0 }];
    let mut slots = [0.0_f64];
    trame::parallel!(fused, &(), &items, &mut slots);
}

// spec: Requirement (Floating point): sin is a std function (libm, which differs between platforms), so an item may not
// call f64::sin. trame::math::f64::sin is the same call on every backend and is accepted (trusted/math_f64_every_item_ok).
struct W {
    x: f64,
}

#[trame::item(key = 0)]
fn wave(w: &W, slot: &mut f64, _cx: &()) {
    *slot = w.x.sin(); //~ ERROR: [std] item
}

fn main() {
    let items = [W { x: 1.0 }];
    let mut slots = [0.0_f64];
    trame::parallel!(wave, &(), &items, &mut slots);
}

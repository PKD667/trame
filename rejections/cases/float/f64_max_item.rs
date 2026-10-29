// spec: Requirement (Floating point): max and min are not named as bit-exact (the sign of a zero result is unspecified),
// so an item may not call f64::max. Its value comes from trame::math, if it is needed there.
struct W {
    x: f64,
}

#[trame::item(key = 0)]
fn clamp_up(w: &W, slot: &mut f64, _cx: &()) {
    *slot = w.x.max(1.0); //~ ERROR: [allow-list] item
}

fn main() {
    let items = [W { x: -2.0 }];
    let mut slots = [0.0_f64];
    trame::parallel!(clamp_up, &(), &items, &mut slots);
}

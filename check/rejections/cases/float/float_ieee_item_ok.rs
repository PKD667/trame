//@check-pass
// spec: Requirement (Floating point): abs and is_finite are named there as bit-exact, so allowed at every level.
// The comparison and the subtraction are operators. No libm function is called.
struct W {
    x: f64,
}

#[trame::item(key = 0)]
fn clamp_up(w: &W, slot: &mut f64, _cx: &()) {
    let y = (w.x - 1.0).abs();
    *slot = if y.is_finite() && y < 10.0 { y } else { 0.0 };
}

fn main() {
    let items = [W { x: -2.0 }];
    let mut slots = [0.0_f64];
    trame::parallel!(clamp_up, &(), &items, &mut slots);
}

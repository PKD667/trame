//@check-pass
// spec: Requirement (Floating point): abs, copysign, the classification predicates, to_bits and from_bits are named
// there as bit-exact, so an item may use them at every level. Comparisons and the operators are allowed too.
struct W {
    x: f64,
}

#[trame::item(key = 0)]
fn bits(w: &W, slot: &mut u64, _cx: &()) {
    let a = w.x.abs();
    let b = a.copysign(-1.0);
    let ok = a.is_finite() && !b.is_nan() && b.is_sign_negative();
    *slot = if ok { f64::from_bits(a.to_bits()).to_bits() } else { 0 };
}

fn main() {
    let items = [W { x: -2.0 }];
    let mut slots = [0_u64];
    trame::parallel!(bits, &(), &items, &mut slots);
}

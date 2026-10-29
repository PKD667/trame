//@check-pass
// spec: Requirement (Floating point): the numeric functions trame needs come from trame::math, defined by one portable
// code. trame::math::f64::sqrt is such a function, so an item may call it (driver/README.md: trame's own bodies are trusted).
struct W {
    x: f64,
}

#[trame::item(key = 0)]
fn root(w: &W, slot: &mut f64, _cx: &()) {
    *slot = trame::math::f64::sqrt(w.x);
}

fn main() {
    let items = [W { x: 4.0 }];
    let mut slots = [0.0_f64];
    trame::parallel!(root, &(), &items, &mut slots);
}

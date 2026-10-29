// spec: Requirement (Floating point): sqrt is a std function, not one of the bit-exact operations. Every other numeric
// function comes from trame::math, so an item may not call f64::sqrt.
struct W {
    x: f64,
}

#[trame::item(key = 0)]
fn root(w: &W, slot: &mut f64, _cx: &()) {
    *slot = w.x.sqrt(); //~ ERROR: [std] item
}

fn main() {
    let items = [W { x: 4.0 }];
    let mut slots = [0.0_f64];
    trame::parallel!(root, &(), &items, &mut slots);
}

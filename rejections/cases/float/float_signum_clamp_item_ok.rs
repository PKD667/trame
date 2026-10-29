//@check-pass
// spec: Requirement (Floating point): signum, clamp and total_cmp are specified bit for bit (signum of a zero is
// one with its sign, clamp compares and returns one of its arguments, total_cmp orders the bits), so an item may
// call them. max and min stay refused (f64_max_item).
struct W {
    x: f64,
}

#[trame::item(key = 0)]
fn sign(w: &W, slot: &mut f64, _cx: &()) {
    let s = w.x.signum() * w.x.clamp(-1.0, 1.0);
    *slot = if s.total_cmp(&0.0).is_lt() { -s } else { s };
}

fn main() {
    let items = [W { x: -2.0 }];
    let mut slots = [0.0_f64];
    trame::parallel!(sign, &(), &items, &mut slots);
}

// spec: Requirement (All levels): no unsafe. mem::transmute is an unsafe function.
struct W {
    n: u32,
}

#[trame::item(key = 0)]
fn go(w: &W, slot: &mut u32, _cx: &()) {
    *slot = unsafe { std::mem::transmute::<f32, u32>(w.n as f32) }; //~ ERROR: [unsafe] item
}

fn main() {
    let items = [W { n: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(go, &(), &items, &mut slots);
}

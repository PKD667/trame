// spec: Requirement (All levels): no indirect calls. An item calls through a fn pointer.
fn f(x: u32) -> u32 { x + 1 }

struct W {
    n: u32,
}

#[trame::item(key = 0)]
fn go(w: &W, slot: &mut u32, _cx: &()) {
    let g: fn(u32) -> u32 = f;
    *slot = g(w.n); //~ ERROR: [indirect] item
}

fn main() {
    let items = [W { n: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(go, &(), &items, &mut slots);
}

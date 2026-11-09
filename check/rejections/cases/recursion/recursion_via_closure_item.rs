// spec: Requirement (Item): no recursion. The item calls itself through Option::map and a closure.
fn r(n: u32) -> u32 {
    if n == 0 { 0 } else { Some(n - 1).map(|k| r(k) + 1).unwrap() } //~ ERROR: [recursion] item
}

struct W {
    n: u32,
}

#[trame::item(key = 0)]
fn go(w: &W, slot: &mut u32, _cx: &()) {
    *slot = r(w.n);
}

fn main() {
    let items = [W { n: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(go, &(), &items, &mut slots);
}

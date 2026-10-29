// spec: Requirement (All levels): no unsafe. The fn item `bad` is passed to core's Iterator::map, which calls it.
fn bad(x: &u32) -> u32 {
    let p = x as *const u32;
    unsafe { *p }
    //~^ ERROR: [unsafe] item
    //~| ERROR: [unsafe] item
}

struct W {
    n: u32,
}

#[trame::item(key = 0)]
fn go(w: &W, slot: &mut u32, _cx: &()) {
    let v = [w.n, 2];
    *slot = v.iter().map(bad).sum();
}

fn main() {
    let items = [W { n: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(go, &(), &items, &mut slots);
}

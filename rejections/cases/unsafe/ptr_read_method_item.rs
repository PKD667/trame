// spec: Requirement (All levels): no unsafe, and no call the deny list (driver/lists/deny.txt) names. <*const T>::read is
// an unsafe method, reached through method call syntax on a raw pointer made from a safe reference with `as`.
// Refused [unsafe] item (two): the unsafe block, and the call to the unsafe method read.
// Refused [deny] item: read is denied by core::ptr::* (driver/lists/deny.txt, A_I.txt).
struct W {
    n: u32,
}

#[trame::item(key = 0)]
fn go(w: &W, slot: &mut u32, _cx: &()) {
    let p = &w.n as *const u32;
    *slot = unsafe { p.read() };
    //~^ ERROR: [unsafe] item
    //~| ERROR: [unsafe] item
    //~| ERROR: [deny] item
}

fn main() {
    let items = [W { n: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(go, &(), &items, &mut slots);
}

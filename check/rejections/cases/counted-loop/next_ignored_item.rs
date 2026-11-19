// spec: Definition (Counted loop): a loop is counted when it is a `for` over a range, slice or array. This loop calls
// `next` on a range at its head and ignores the answer, so it never leaves: probe for a hole where the test only looks at
// the call, not at the exit on None.
struct W {
    x: u32,
}

#[trame::item(key = 0)]
fn spin(w: &W, slot: &mut u32, _cx: &()) {
    let mut it = 0..w.x;
    loop {
        let _ = it.next(); //~ ERROR: [counted-loop] item
    }
}

fn main() {
    let items = [W { x: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(spin, &(), &items, &mut slots);
}

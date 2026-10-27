//! expect: refused [counted-loop] item
// spec: Requirement (Item): every loop is counted. An item loops with while, which is not a counted loop.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn count(x: &Work, slot: &mut u32, _cx: &()) {
    let mut i = 0u32;
    while i < x.by {
        i += 1;
    }
    *slot = i;
}

fn main() {
    let items = [Work { index: 0, by: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(count, &(), &items, &mut slots);
}

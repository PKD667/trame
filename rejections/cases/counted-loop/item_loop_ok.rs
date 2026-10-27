//! expect: accepted
// spec: Requirement (Item): every loop is counted, and a counted loop is allowed. A `for` over a range in an
// item is accepted (the fixed bound is the loop's own).
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn total(x: &Work, slot: &mut u32, _cx: &()) {
    for i in 0..4u32 {
        *slot += i * x.by;
    }
}

fn main() {
    let items = [Work { index: 0, by: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(total, &(), &items, &mut slots);
}

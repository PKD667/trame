// spec: Requirement (Item): an item neither recurses. A function that calls itself is refused.
fn down(n: u32) -> u32 {
    if n == 0 { 0 } else { down(n - 1) } //~ ERROR: [recursion] item
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn count(x: &Work, slot: &mut u32, _cx: &()) {
    *slot = down(x.by);
}

fn main() {
    let items = [Work { index: 0, by: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(count, &(), &items, &mut slots);
}

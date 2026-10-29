//@check-pass
// spec: Requirement (Item): an item calls only items of A_I. `ne` and `le` are default methods of core's
// PartialEq and PartialOrd, which the user's derived impls do not define. Their bodies are core's, so they are
// matched by their trait path (driver/lists/A_I.txt, core::cmp::*), not refused as a user's own method.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Key(u32);

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn check(x: &Work, slot: &mut u32, _cx: &()) {
    let a = Key(x.by);
    let b = Key(x.index);
    *slot = (a != b) as u32 + (a <= b) as u32;
}

fn main() {
    let items = [Work { index: 0, by: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(check, &(), &items, &mut slots);
}

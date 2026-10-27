//! expect: refused [allow-list] item
// spec: Section 3 (Compile time): a function calls items of core and alloc only from the lists A_I, A_C, A_W.
// core::mem::forget is in no list, so an item may not call it.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn drop_it(x: &Work, slot: &mut u32, _cx: &()) {
    core::mem::forget(x.by);
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(drop_it, &(), &items, &mut slots);
}

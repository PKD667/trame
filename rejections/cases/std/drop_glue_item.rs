// spec: Requirement (All levels): no std. Drop glue counts as a call (spec.md, Requirement (All levels)): the value
// is dropped at the end of the item, and its Drop impl, which calls std, is reached and refused.
struct Loud(u32);

impl Drop for Loud {
    fn drop(&mut self) {
        std::thread::yield_now(); //~ ERROR: [std] item
    }
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn hold(x: &Work, slot: &mut u32, _cx: &()) {
    let l = Loud(x.by);
    *slot = l.0;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(hold, &(), &items, &mut slots);
}

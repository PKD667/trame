//! expect: refused [allocation] item
// spec: Requirement (Item): an item neither allocates nor calls alloc. Vec::push may grow the buffer.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn collect(x: &Work, slot: &mut u32, _cx: &()) {
    let mut v: Vec<u32> = Vec::new();
    v.push(x.by);
    *slot = v[0];
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(collect, &(), &items, &mut slots);
}

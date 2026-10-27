//! expect: refused [allocation] item
// spec: Requirement (Item): an item neither allocates nor calls alloc. Vec::new allocates nothing, but it is a call
// into alloc, so the letter of the rule refuses it, and so do Vec::len and the Vec's drop glue. This is the strict
// reading. Whether it is over-refusal is the open question in rejections/README.md (finding A1).
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn empty(x: &Work, slot: &mut u32, _cx: &()) {
    let v: Vec<u32> = Vec::new();
    *slot = v.len() as u32 + x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(empty, &(), &items, &mut slots);
}

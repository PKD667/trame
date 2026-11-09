//@check-pass
// spec: Requirement (All levels) is about unsafe code. This is the pair of block_item.rs: the same read
// through a reference, which needs no unsafe block, is accepted.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn read(x: &Work, slot: &mut u32, _cx: &()) {
    let r = &x.by;
    *slot = *r;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(read, &(), &items, &mut slots);
}

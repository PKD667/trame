// spec: Requirement (All levels): no unsafe. An item reads through a raw pointer in an unsafe block.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn read(x: &Work, slot: &mut u32, _cx: &()) {
    let p = &x.by as *const u32;
    *slot = unsafe { *p };
    //~^ ERROR: [unsafe] item
    //~| ERROR: [unsafe] item
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(read, &(), &items, &mut slots);
}

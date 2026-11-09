// spec: Requirement (All levels): no globals. An item reads a static.
static COUNT: u32 = 7;

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn read(x: &Work, slot: &mut u32, _cx: &()) {
    *slot = COUNT + x.by; //~ ERROR: [static] item
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(read, &(), &items, &mut slots);
}

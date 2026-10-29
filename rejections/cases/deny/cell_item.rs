// spec: Requirement (All levels): no UnsafeCell, so no interior mutability. An item writes through a Cell.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn bump(x: &Work, slot: &mut u32, _cx: &()) {
    let c = core::cell::Cell::new(x.by); //~ ERROR: [deny] item
    c.set(c.get() + 1);
    //~^ ERROR: [deny] item
    //~| ERROR: [deny] item
    *slot = c.get(); //~ ERROR: [deny] item
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(bump, &(), &items, &mut slots);
}

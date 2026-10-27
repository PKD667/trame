//! expect: refused [effects] item, [allocation] item
// spec: Requirement (Item), Definition (Effect sets): an item performs no effect; barrier is a worker effect.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn wait_all(x: &Work, slot: &mut u32, _cx: &()) {
    let _ = trame::barrier();
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(wait_all, &(), &items, &mut slots);
}

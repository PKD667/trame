//! expect: refused [std] item, [unsafe] item, [allocation] item
// spec: Requirement (All levels): no threads. An item spawns one. The spawn is std, the call is in no allow-list,
// and the dropped JoinHandle's drop glue reaches a raw-pointer drop_in_place (unsafe).
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn spawn_one(x: &Work, slot: &mut u32, _cx: &()) {
    let _ = std::thread::spawn(|| {});
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(spawn_one, &(), &items, &mut slots);
}

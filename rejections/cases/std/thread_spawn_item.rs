//@error-in-other-file: [std] item
//@error-in-other-file: [allocation] item
//@error-in-other-file: [allocation] item
// spec: Requirement (All levels): no threads, and drop glue counts as a call. An item spawns one. The spawn is std, and the
// dropped JoinHandle's drop glue reaches std. The glue's own raw deref is the compiler's, not the item's.
// Refused [std] item: std::thread::spawn. [allocation] item: a drop glue reached from the item calls alloc::sync::Arc::drop, and an
// item neither allocates nor calls alloc (Requirement (Item)).
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn spawn_one(x: &Work, slot: &mut u32, _cx: &()) {
    let _ = std::thread::spawn(|| {}); //~ ERROR: [std] item
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(spawn_one, &(), &items, &mut slots);
}

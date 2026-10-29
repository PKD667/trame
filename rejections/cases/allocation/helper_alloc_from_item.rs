// spec: Requirement (Item): an item neither allocates nor calls alloc. The helper pushes to a Vec: legal for a
// worker (which may allocate), but the item reaches it too, and a function is checked at the strictest level
// that reaches it (driver/README.md). So the helper is refused at the item level.
fn remember(n: u32) -> u32 {
    let mut v: Vec<u32> = Vec::new(); //~ ERROR: [allocation] item
    v.push(n); //~ ERROR: [allocation] item
    v[0]
} //~ ERROR: [allocation] item

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn bump(x: &Work, slot: &mut u32, _cx: &()) {
    *slot = remember(x.by);
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(bump, &(), &items, &mut slots);
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let _ = remember(1);
    });
}

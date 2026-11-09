// spec: Requirement (Item): an item neither allocates nor calls alloc. helper pushes to a Vec.
// crates/driver/README.md: a function reached from several roots is checked at the strictest level that reaches it.
// helper is reached from an item (bump) and from a worker (main), so the item level applies, and helper is refused.
// Refused [allocation] item: Vec::new, Vec::push and the drop. The worker use alone is accepted: strictest_level_worker_ok.rs.
fn helper() {
    let mut v: Vec<u32> = Vec::new(); //~ ERROR: [allocation] item
    v.push(1); //~ ERROR: [allocation] item
} //~ ERROR: [allocation] item

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn bump(x: &Work, slot: &mut u32, _cx: &()) {
    helper();
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(bump, &(), &items, &mut slots);
    trame::__check::worker(&|| helper());
}

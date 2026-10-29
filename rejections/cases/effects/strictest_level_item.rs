// spec: Requirement (Item), Definition (Effect sets): an item performs no effect. helper calls barrier, a worker effect.
// driver/README.md:21: a function reached from several roots is checked at the strictest level that reaches it.
// helper is reached from an item (bump) and from a worker (main), so the item level applies, and helper is refused.
// Refused [effects] item: the call to trame::barrier. [allocation] item: the Arc that barrier returns is dropped (Requirement (Item)).
// The worker use alone is accepted: strictest_level_worker_ok.rs.
fn helper() {
    let _ = trame::barrier();
    //~^ ERROR: [effects] item
    //~| ERROR: [allocation] item
}

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
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| helper());
}

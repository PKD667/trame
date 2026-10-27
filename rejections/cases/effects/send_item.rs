//! expect: refused [effects] item
// spec: Requirement (Item), Definition (Effect sets): Sigma_I is empty. An item performs no effect; send is one.
#[trame::message]
struct Msg {
    n: u32,
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn sending(x: &Work, slot: &mut u32, _cx: &()) {
    let _ = trame::send(trame::LEADER, Msg { n: x.by });
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(sending, &(), &items, &mut slots);
}

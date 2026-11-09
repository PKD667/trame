// spec: Requirement (Item), Definition (Effect sets): Sigma_I is empty. An item receives a message, which is an effect.
#[trame::message]
struct Msg {
    n: u32,
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn receiving(x: &Work, slot: &mut u32, _cx: &()) {
    if let Some(m) = trame::recv::<Msg>() { //~ ERROR: [effects] item
        *slot = m.n + x.by;
    }
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(receiving, &(), &items, &mut slots);
}

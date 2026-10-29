//@check-pass
// spec: Section 3 (Compile time): the pair of forget_item.rs. core::mem::swap is in A_I, so an item may call
// it. The same shape of call, with a listed function, is accepted.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn swap_it(x: &Work, slot: &mut u32, _cx: &()) {
    let mut a = x.by;
    let mut b = 0u32;
    core::mem::swap(&mut a, &mut b);
    *slot = b;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(swap_it, &(), &items, &mut slots);
}

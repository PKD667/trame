//@check-pass
// spec: Requirement (All levels): no unsafe. Reading the value a Box holds is a safe deref of a Box (Box is not a raw
// pointer, though MIR lowers its deref to one). An item may read it: it allocates nothing.
struct W {
    b: Box<u32>,
}

#[trame::item(key = 0)]
fn read(w: &W, slot: &mut u32, _cx: &()) {
    *slot = *w.b + 1;
}

fn main() {
    let items = [W { b: Box::new(4) }];
    let mut slots = [0_u32];
    trame::parallel!(read, &(), &items, &mut slots);
}

//! expect: error build with cargo trame
//! runner: plain cargo
// spec: Section 5.4 (Items and parallel!): parallel! needs the driver's checks. Without --cfg trame_checked the
// macro is a compile error that says how to build. Run under plain cargo (run.sh does this: runner = plain cargo).
#[trame::item(key = x.index as usize)]
fn bump(x: &u32, slot: &mut u32, _cx: &()) {
    *slot = *x;
}

fn main() {
    let items = [1_u32];
    let mut slots = [0_u32];
    trame::parallel!(bump, &(), &items, &mut slots);
}

//! runner: plain cargo
// spec: Requirement (Checked builds): without --cfg trame_checked, parallel! expands to compile_error!. The error says
// how to build. This case runs under plain cargo (see the runner line), so the flag is not set.
#[trame::item(key = *x as usize)]
fn bump(x: &u32, slot: &mut u32, _cx: &()) {
    *slot = *x;
}

fn main() {
    let items = [1_u32];
    let mut slots = [0_u32];
    trame::parallel!(bump, &(), &items, &mut slots); //~ ERROR: build with cargo trame
}

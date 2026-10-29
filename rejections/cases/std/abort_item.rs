// spec: Requirement (All levels): no std. std::process::abort is std. (A panic is an abort of the launch, and
// is allowed: see option_expect_step_ok.rs.)
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn check(x: &Work, slot: &mut u32, _cx: &()) {
    if x.by > 9 {
        std::process::abort(); //~ ERROR: [std] item
    }
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(check, &(), &items, &mut slots);
}

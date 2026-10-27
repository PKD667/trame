//! expect: refused [std] item, [unsafe] item
// spec: Requirement (All levels): no std. An item prints, which calls std. The unsafe rule also fires on core's
// `fmt::Arguments::new`, which the format_args! expansion calls (rejections/README.md, finding D1).
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn show(x: &Work, slot: &mut u32, _cx: &()) {
    std::println!("{}", x.by);
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(show, &(), &items, &mut slots);
}

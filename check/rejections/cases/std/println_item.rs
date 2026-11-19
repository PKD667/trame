// spec: Requirement (All levels): no std. An item prints, which calls std. The format_args! expansion's unsafe
// `fmt::Arguments::new` is not judged (README.md, finding D1, as corrected again).
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn show(x: &Work, slot: &mut u32, _cx: &()) {
    std::println!("{}", x.by);
    //~^ ERROR: [std] item
    *slot = x.by;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(show, &(), &items, &mut slots);
}

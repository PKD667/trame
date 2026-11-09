//@check-pass
// spec: Requirement (All levels) forbids FFI, which is a call into code that is not Rust. This is the pair
// of extern_item.rs: an `extern "C"` function defined in Rust is an ordinary Rust function, so it is accepted.
extern "C" fn bump(x: u32) -> u32 {
    x + 1
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn absolute(x: &Work, slot: &mut u32, _cx: &()) {
    *slot = bump(x.by);
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(absolute, &(), &items, &mut slots);
}

//! expect: refused [ffi] item, [unsafe] item
// spec: Requirement (All levels): no FFI. The item calls abs from the C library. Rust makes the call an unsafe
// block, so the unsafe rule reports it too: both are true, and both are named.
unsafe extern "C" {
    fn abs(x: i32) -> i32;
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn absolute(x: &Work, slot: &mut i32, _cx: &()) {
    *slot = unsafe { abs(x.by as i32) };
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_i32];
    trame::parallel!(absolute, &(), &items, &mut slots);
}

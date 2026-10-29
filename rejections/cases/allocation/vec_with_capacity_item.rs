// spec: Requirement (Item): an item neither allocates nor calls alloc. Vec::with_capacity allocates.
struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn reserve(x: &Work, slot: &mut u32, _cx: &()) {
    let v: Vec<u32> = Vec::with_capacity(4); //~ ERROR: [allocation] item
    *slot = v.capacity() as u32 + x.by; //~ ERROR: [allocation] item
} //~ ERROR: [allocation] item

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(reserve, &(), &items, &mut slots);
}

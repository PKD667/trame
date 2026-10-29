//@check-pass
// spec: Requirement (Item): an item neither recurses nor allocates. Two helpers call the same leaf: the call graph
// is a DAG (a diamond), not a cycle, so no recursion. A walk that marks a function as its own caller on a shared
// callee would refuse this.
fn leaf(n: u32) -> u32 {
    n * 3
}

fn left(n: u32) -> u32 {
    leaf(n) + 1
}

fn right(n: u32) -> u32 {
    leaf(n) + 2
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn both(x: &Work, slot: &mut u32, _cx: &()) {
    *slot = left(x.by) + right(x.by);
}

fn main() {
    let items = [Work { index: 0, by: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(both, &(), &items, &mut slots);
}

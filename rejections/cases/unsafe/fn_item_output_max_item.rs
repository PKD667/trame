// spec: Requirement (All levels): no unsafe code. `mk` is a safe fn item returning X; `max` calls X's Ord::cmp, which
// holds unsafe code. X is in no generic argument of `max` (only in `mk`'s signature), so the reach walk must follow
// the signature of a fn item (scan.rs, reach_types). Before that fix the item was accepted.
use core::cmp::Ordering;

#[derive(PartialEq, Eq)]
struct X(u32);

impl PartialOrd for X {
    fn partial_cmp(&self, o: &X) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for X {
    fn cmp(&self, o: &X) -> Ordering {
        let p = &self.0 as *const u32;
        let _ = unsafe { *p }; //~ ERROR: [unsafe] item
        self.0.cmp(&o.0)
    }
}

fn mk(i: u32) -> X {
    X(i)
}

struct W {
    index: u32,
}

#[trame::item(key = x.index as usize)]
fn pick(x: &W, slot: &mut u32, _cx: &()) {
    *slot = (0..3u32).map(mk).max().unwrap().0;
}

fn main() {
    let items = [W { index: 0 }];
    let mut slots = [0_u32];
    trame::parallel!(pick, &(), &items, &mut slots);
}

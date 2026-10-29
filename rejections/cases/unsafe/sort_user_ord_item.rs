// spec: Requirement (All levels): no unsafe. sort_unstable is core code that calls the user's Ord::cmp.
#[derive(PartialEq, Eq)]
struct K(u32);

impl PartialOrd for K {
    fn partial_cmp(&self, o: &K) -> Option<core::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for K {
    fn cmp(&self, o: &K) -> core::cmp::Ordering {
        let p = &self.0 as *const u32;
        unsafe { (*p).cmp(&o.0) }
        //~^ ERROR: [unsafe] item
        //~| ERROR: [unsafe] item
    }
}

struct W {
    n: u32,
}

#[trame::item(key = 0)]
fn go(w: &W, slot: &mut u32, _cx: &()) {
    let mut v = [K(w.n), K(1)];
    v.sort_unstable();
    *slot = v[0].0;
}

fn main() {
    let items = [W { n: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(go, &(), &items, &mut slots);
}

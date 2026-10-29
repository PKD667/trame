// spec: Requirement (All levels): no unsafe, and a user Iterator impl is user code. Iterator::sum is core code that
// calls the user's next(); next holds unsafe code.
// Refused [unsafe] item (two): the unsafe block and the dereference of a raw pointer, both in It::next.
// Refused [unbounded] item: sum passes It, a user iterator, to core code, which would drive it without a bound (Definition (Counted loop)).
struct It(u32);

impl Iterator for It {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        let p = &self.0 as *const u32;
        if self.0 == 0 { None } else { self.0 -= 1; Some(unsafe { *p }) }
        //~^ ERROR: [unsafe] item
        //~| ERROR: [unsafe] item
    }
}

struct W {
    n: u32,
}

#[trame::item(key = 0)]
fn total(w: &W, slot: &mut u32, _cx: &()) {
    *slot = It(w.n).sum(); //~ ERROR: [unbounded] item
}

fn main() {
    let items = [W { n: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(total, &(), &items, &mut slots);
}

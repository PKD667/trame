// spec: Requirement (Item): no recursion. Drop glue is a call: X::drop drops a fresh X, which calls X::drop again.
struct X(u32);

impl Drop for X {
    fn drop(&mut self) {
        if self.0 > 0 {
            let _ = X(self.0 - 1); //~ ERROR: [recursion] item
        }
    }
}

struct W {
    n: u32,
}

#[trame::item(key = 0)]
fn go(w: &W, slot: &mut u32, _cx: &()) {
    let _x = X(w.n);
    *slot = 1;
}

fn main() {
    let items = [W { n: 3 }];
    let mut slots = [0_u32];
    trame::parallel!(go, &(), &items, &mut slots);
}

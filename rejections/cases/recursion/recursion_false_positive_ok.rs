//@check-pass
// spec: Requirement (Item): no recursion. Mul::mul does not call itself; `.expect` on a Result<Span, ()> makes
// the driver walk Span's core-trait impls (reach, scan.rs), which lists Mul::mul as a callee of itself.
#[derive(Clone, Copy)]
struct Span(u64);

fn checked(x: f64) -> Result<Span, ()> {
    if x >= 0.0 { Ok(Span(x as u64)) } else { Err(()) }
}

impl core::ops::Mul<f64> for Span {
    type Output = Span;
    fn mul(self, k: f64) -> Span {
        checked(self.0 as f64 * k).expect("in range")
    }
}

struct W {
    n: u64,
}

#[trame::item(key = 0)]
fn scale(w: &W, slot: &mut u64, _cx: &()) {
    *slot = (Span(w.n) * 2.0).0;
}

fn main() {
    let items = [W { n: 3 }];
    let mut slots = [0_u64];
    trame::parallel!(scale, &(), &items, &mut slots);
}

// spec: Requirement (All levels): no unsafe. [X; 2] == [X; 2] calls core's slice PartialEq, which calls X::eq; X::eq
// holds unsafe code.
struct X(u32);

impl PartialEq for X {
    fn eq(&self, o: &X) -> bool {
        let p = &self.0 as *const u32;
        unsafe { *p == o.0 }
        //~^ ERROR: [unsafe] item
        //~| ERROR: [unsafe] item
    }
}

struct W {
    a: [X; 2],
}

#[trame::item(key = 0)]
fn cmp(w: &W, slot: &mut u32, _cx: &()) {
    *slot = (w.a == [X(1), X(2)]) as u32;
}

fn main() {
    let items = [W { a: [X(1), X(2)] }];
    let mut slots = [0_u32];
    trame::parallel!(cmp, &(), &items, &mut slots);
}

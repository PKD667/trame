//! expect: refused [effects] item
// spec: Requirement (Item), Definition (Effect sets): an item performs no effect, and concurrent! is one. A
// concurrent! in an item is refused by its effect. The driver reports only that rule here.
#[trame::message]
struct Msg {
    n: u32,
}

struct Queue {
    n: u32,
}

struct Inner;

#[trame::process(receives = Msg)]
impl Inner {
    fn step(&mut self, q: &mut Queue) -> trame::Status {
        q.n += 1;
        trame::Status::Done
    }
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn run_one(x: &Work, slot: &mut u32, _cx: &()) {
    let mut q = Queue { n: x.by };
    let _ = trame::concurrent!(Inner; q);
    *slot = q.n;
}

fn main() {
    let items = [Work { index: 0, by: 1 }];
    let mut slots = [0_u32];
    trame::parallel!(run_one, &(), &items, &mut slots);
}

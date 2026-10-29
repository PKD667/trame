//@check-pass
// spec: Definition (Effect sets): Sigma_C contains parallel. This is the pair of parallel_item.rs: the same
// parallel!, in a step.
#[trame::message]
struct Msg {
    n: u32,
}

struct Work {
    index: u32,
    by: u32,
}

#[trame::item(key = x.index as usize)]
fn bump(x: &Work, slot: &mut u32, _cx: &()) {
    *slot += x.by;
}

struct Queue {
    n: u32,
}

struct P;

#[trame::process(receives = Msg)]
impl P {
    fn step(&mut self, q: &mut Queue) -> trame::Status {
        let items = [Work { index: 0, by: 1 }];
        let mut slots = [0_u32];
        trame::parallel!(bump, &(), &items, &mut slots);
        q.n = slots[0];
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

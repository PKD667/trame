// spec: Requirement (All levels): no globals, and no mutable globals (static mut is a global with unsafe
// access). A step reads one inside an unsafe block.
static mut COUNT: u32 = 7;

#[trame::message]
struct Job {
    id: u32,
}

struct Queue {
    n: u32,
}

struct P;

#[trame::process(receives = Job)]
impl P {
    fn step(&mut self, q: &mut Queue) -> trame::Status {
        q.n = unsafe { COUNT };
        //~^ ERROR: [unsafe] step
        //~| ERROR: [static] step
        //~| ERROR: [unsafe] step
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

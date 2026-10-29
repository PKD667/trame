// spec: Requirement (All levels): no UnsafeCell, so no interior mutability. A step uses a RefCell.
//@error-in-other-file: [deny] step
//@error-in-other-file: [deny] step
//@error-in-other-file: [deny] step
//@error-in-other-file: [deny] step
use core::cell::RefCell;

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
        let r = RefCell::new(q.n); //~ ERROR: [deny] step
        *r.borrow_mut() += 1;
        //~^ ERROR: [deny] step
        //~| ERROR: [deny] step
        //~| ERROR: [deny] step
        q.n = *r.borrow();
        //~^ ERROR: [deny] step
        //~| ERROR: [deny] step
        //~| ERROR: [deny] step
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

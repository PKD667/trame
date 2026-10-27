//! expect: refused [allow-list] step, [std] step, [unsafe] step
// spec: Requirement (All levels): no threads. A step spawns one with std::thread::spawn. Three rules hold: the call
// is std; it is in no allow-list; and dropping the JoinHandle (drop glue, spec.md "Drop glue counts as a call")
// reaches std and a raw-pointer drop_in_place.
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
        let _ = std::thread::spawn(|| {});
        q.n += 1;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

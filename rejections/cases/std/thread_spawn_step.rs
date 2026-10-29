// spec: Requirement (All levels): no threads. A step spawns one with std::thread::spawn. Two rules hold: the call
// is std, and it is in no allow-list. Dropping the JoinHandle (drop glue, spec.md "Drop glue counts as a call")
// also reaches std. The glue's own raw deref is the compiler's, not the step's.
//@error-in-other-file: [std] step
//@error-in-other-file: [allow-list] step
//@error-in-other-file: [allow-list] step
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
        let _ = std::thread::spawn(|| {}); //~ ERROR: [std] step
        q.n += 1;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 0 };
    let _ = trame::concurrent!(P; q);
}

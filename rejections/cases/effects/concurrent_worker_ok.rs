//! expect: accepted
// spec: Definition (Effect sets): Sigma_W contains concurrent. This is the pair of concurrent_step.rs: the same
// concurrent!, in a worker.
#[trame::message]
struct Job {
    id: u32,
}

struct Queue {
    n: u32,
}

struct Inner;

#[trame::process(receives = Job)]
impl Inner {
    fn step(&mut self, q: &mut Queue) -> trame::Status {
        q.n += 1;
        trame::Status::Done
    }
}

fn main() {
    let mut t = trame::Topology::new();
    let _ = t.worker(|_cx| {
        let mut q = Queue { n: 0 };
        let _ = trame::concurrent!(Inner; q);
    });
}

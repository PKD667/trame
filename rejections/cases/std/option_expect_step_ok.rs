//@check-pass
// spec: Definition (Abort): a panic aborts the launch; it is not an effect and not std. Option::expect and
// assert_eq! reach a panic, so both are accepted in a step.
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
        let x: Option<u32> = Some(q.n);
        let y = x.expect("present");
        assert_eq!(y, q.n);
        trame::Status::Done
    }
}

fn main() {
    let mut q = Queue { n: 3 };
    let _ = trame::concurrent!(P; q);
}

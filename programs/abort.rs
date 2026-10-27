//! abort: worker 0 panics before it sends anything. Worker 1 waits for a frame that never comes, so
//! only the abort can end it. The leader reports Abort for both workers, and the run does not hang.

use trame::{Context, Outcome, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Never {
    n: u32,
}

/// The cell the process shares. It holds nothing.
struct Shared;

/// Waits for a `Never` frame, which worker 0 does not send.
struct Waiter;

#[trame::process(receives = Never)]
impl Waiter {
    fn step(&mut self, _s: &mut Shared) -> Status {
        match trame::recv::<Never>() {
            Some(_) => Status::Done,
            None => Status::wait().frame(),
        }
    }
}

fn fails(_cx: &Context) {
    panic!("worker 0 fails on purpose");
}

fn waits(_cx: &Context) {
    let mut shared = Shared;
    let r = trame::concurrent!(Waiter; shared);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let a = t.worker(fails);
    let b = t.worker(waits);
    t.route::<Never>(a, b, 1);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

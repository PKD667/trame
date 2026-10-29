//! abort: worker 0 panics before it sends anything. Worker 1 waits for a frame that never comes, so
//! only the abort can end it. The leader is blocked in `Run::wait`, and the panic of worker 0 ends the
//! leader's call: it panics with `trame: worker 0 panicked: worker 0 fails on purpose` (abort.panics).
//! abort_join.rs is the same with the leader in `join`.

use trame::{Context, Status, Topology};

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
    // Blocks until the run changes for the leader; the abort is a change, and it panics the leader.
    loop {
        run.wait();
    }
}

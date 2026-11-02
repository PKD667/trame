//! abort_join: worker 0 panics in its first step, before it sends anything. Worker 1 waits for a frame
//! that never comes, so only the abort can end it. The leader is blocked in `Run::join`, and the panic of
//! worker 0 ends the leader's call: it panics with `trame: worker 0 panicked: worker 0 fails on purpose`
//! (abort_join.panics). abort.rs is the same with the leader in `Run::wait`.

use trame::{Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Never {
    n: u32,
}

/// Worker 0's state: nothing, its one step panics.
#[trame::state(steps(fails))]
struct Fails {}

#[trame::step(Fails)]
fn fails() -> Status {
    panic!("worker 0 fails on purpose");
}

/// Worker 1's state: nothing, its one step waits for a frame.
#[trame::state(steps(waits))]
struct Waiter {}

/// Waits for a `Never` frame, which worker 0 does not send.
#[trame::step(Waiter, receives = Never)]
fn waits() -> Status {
    match trame::recv::<Never>() {
        Some(_) => Status::Done,
        None => Status::wait().frame(),
    }
}

fn main() {
    let mut t = Topology::new();
    let a = t.worker();
    t.set(&a, move || Fails {});
    let b = t.worker();
    t.set(&b, move || Waiter {});
    t.route::<Never>(a.id(), b.id(), 1);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let _outcomes = run.join();
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

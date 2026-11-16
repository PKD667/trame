//! leader_panics: the leader panics before `join`, while a worker waits for a frame the leader will never
//! send. Spec.md 5, Abort: a panic ends the run, so the worker unwinds and the launch ends with the leader's
//! panic (leader_panics.panics). On several hosts, before the fix in the backend's leader, the process hung
//! at exit instead, because a relay thread waited for END (the tree has none now).

use trame::{Leader, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Never {
    n: u32,
}

/// The state: nothing. Its one step waits.
#[trame::state(steps(waiter))]
struct Waiter {}

/// Waits for a `Never` frame from the leader, which the leader never sends.
#[trame::step(Waiter, receives = Never)]
fn waiter() -> Status {
    match trame::recv::<Never>() {
        Some(Never { n }) => {
            println!("never {n}");
            Status::Done
        }
        None => Status::wait().frame(),
    }
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker();
    t.set(&w, move || Waiter {});
    t.route::<Never>(Leader, w.id(), 1);
    let _run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    panic!("trame: leader panics on purpose");
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

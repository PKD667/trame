//! leader_stuck: two workers return at once and send nothing. The leader waits for a `Never` frame that no
//! one sends. Its wait can never return: the seq backend panics the call with
//! `trame: deadlock: the leader waits and nothing can arrive` (leader_stuck.panics; spec.md 5, Deadlock).
//! On MPI the leaders' detector shows the same (backends/mpi/detect.rs), and the leader panics alike.

use trame::{Leader, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Never {
    n: u32,
}

/// The state: nothing. Its one step is done at once.
#[trame::state(steps(quick))]
struct Quick {}

#[trame::step(Quick)]
fn quick() -> Status {
    Status::Done
}

fn main() {
    let mut t = Topology::new();
    let a = t.worker();
    t.set(&a, move || Quick {});
    let _b = t.worker();
    t.set(&_b, move || Quick {});
    t.route::<Never>(a.id(), Leader, 1);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    loop {
        if let Some(Never { n }) = run.recv::<Never>() {
            println!("never {n}");
            return;
        }
        run.wait();
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

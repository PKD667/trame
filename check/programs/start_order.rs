//! start_order: one worker with two steps and no fields, so both are ready at once. Both have weight 0,
//! and neither has started: the tie goes to the earlier declared one (sched.rs `choose`: least recently
//! started, ties to the lower index). Each step reports its id to the leader over one route, so the
//! leader sees the order of the starts.

use trame::{Leader, Outcome, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Started {
    id: u32,
}

#[trame::state(steps(first, second))]
struct Pair {}

#[trame::step(Pair)]
fn first() -> Status {
    assert!(trame::send(Leader, Started { id: 1 }).is_ok(), "report did not fit");
    Status::Done
}

#[trame::step(Pair)]
fn second() -> Status {
    assert!(trame::send(Leader, Started { id: 2 }).is_ok(), "report did not fit");
    Status::Done
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker();
    t.set(&w, move || Pair {});
    t.route::<Started>(w.id(), Leader, 2);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    for _ in 0..2 {
        let s = loop {
            if let Some(s) = run.recv::<Started>().map(|s| s.value) {
                break s;
            }
            run.wait();
        };
        println!("started {}", s.id);
    }
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

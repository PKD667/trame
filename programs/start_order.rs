//! start_order: one worker with two steps and no fields, so both are ready at once. Both have weight 0,
//! and neither has started: the tie goes to the earlier declared one (sched.rs `choose`: least recently
//! started, ties to the lower index). Each step reports its id to the leader over one route, so the
//! leader sees the order of the starts.

use trame::{LEADER, Outcome, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Started {
    id: u32,
}

#[trame::state(steps(first, second))]
struct Pair {}

#[trame::step(Pair)]
fn first() -> Status {
    assert!(trame::send(LEADER, Started { id: 1 }).is_ok(), "report did not fit");
    Status::Done
}

#[trame::step(Pair)]
fn second() -> Status {
    assert!(trame::send(LEADER, Started { id: 2 }).is_ok(), "report did not fit");
    Status::Done
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(Pair {});
    t.route::<Started>(w.id(), LEADER, 2);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    for _ in 0..2 {
        let s = loop {
            if let Some(s) = run.recv::<Started>() {
                break s;
            }
            run.wait();
        };
        println!("started {}", s.id);
    }
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

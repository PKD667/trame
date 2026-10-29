//! start_order: one worker runs two processes with no cells, so both are ready at once. Start picks the
//! earlier declared one (spec.md 5.3: "the earliest declared one wins"). Each step reports its id to the
//! leader over one route, so the leader sees the order of the starts.

use trame::{Context, LEADER, Outcome, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Started {
    id: u32,
}

struct First;

#[trame::process]
impl First {
    fn step(&mut self) -> Status {
        assert!(trame::send(LEADER, Started { id: 1 }).is_ok(), "report did not fit");
        Status::Done
    }
}

struct Second;

#[trame::process]
impl Second {
    fn step(&mut self) -> Status {
        assert!(trame::send(LEADER, Started { id: 2 }).is_ok(), "report did not fit");
        Status::Done
    }
}

fn worker(_cx: &Context) {
    let r = trame::concurrent!(First, Second;);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(worker);
    t.route::<Started>(w, LEADER, 2);
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

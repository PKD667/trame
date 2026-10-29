//! leader_stuck: two workers return at once and send nothing. The leader waits for a `Never` frame that no
//! one sends. Its wait can never return: the seq backend panics the call with
//! `trame: deadlock: the leader waits and nothing can arrive` (leader_stuck.panics; spec.md 5, Deadlock).
//! Seq only: it has no row in backends/mpi/deploy/ranks.txt, so it does not run on MPI (on MPI the leader would hang).

use trame::{Context, LEADER, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Never {
    n: u32,
}

fn quick(_cx: &Context) {}

fn main() {
    let mut t = Topology::new();
    let a = t.worker(quick);
    let _b = t.worker(quick);
    t.route::<Never>(a, LEADER, 1);
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

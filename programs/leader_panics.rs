//! leader_panics: the leader panics before `join`, while a worker waits for a frame the leader will never
//! send. Spec.md 5, Abort: a panic ends the run, so the worker unwinds and the launch ends with the leader's
//! panic (leader_panics.panics). On several nodes, before the fix in backends/mpi/leader.rs, the process hung
//! at exit instead, because the relays waited for END.

use trame::{Context, LEADER, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Never {
    n: u32,
}

/// Waits for a `Never` frame from the leader, which the leader never sends.
struct Waiter;

#[trame::process(receives = Never)]
impl Waiter {
    fn step(&mut self) -> Status {
        match trame::recv::<Never>() {
            Some(Never { n }) => {
                println!("never {n}");
                Status::Done
            }
            None => Status::wait().frame(),
        }
    }
}

fn waits(_cx: &Context) {
    let r = trame::concurrent!(Waiter;);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(waits);
    t.route::<Never>(LEADER, w, 1);
    let _run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    panic!("trame: leader panics on purpose");
}

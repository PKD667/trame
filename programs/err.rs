//! err: one worker runs two processes. `Fails` (priority 0) returns an error at its first step. `Idle`
//! (priority 1) waits on a cell that nothing shares, so it could only be woken by a step that never runs.
//! `concurrent!` must answer `Fails`'s error at once, and `Idle` must never step again. The worker reports
//! the code it got back to the leader; the run returns normally.

use trame::{Context, LEADER, Outcome, Status, Topology};

#[derive(Debug, PartialEq, Eq)]
struct Boom(u32);

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    code: u32,
}

struct Fails;

#[trame::process]
impl Fails {
    fn step(&mut self) -> Result<Status, Boom> {
        Err(Boom(7))
    }
}

struct Idle;

#[trame::process]
impl Idle {
    fn step(&mut self) -> Result<Status, Boom> {
        Ok(Status::wait())
    }
}

fn worker(_cx: &Context) {
    match trame::concurrent!(Fails, Idle;) {
        Err(Boom(code)) => assert!(trame::send(LEADER, Report { code }).is_ok(), "report did not fit"),
        Ok(()) => panic!("concurrent! answered Ok, but a process returned Err"),
    }
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(worker);
    t.route::<Report>(w, LEADER, 2);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let report = loop {
        if let Some(r) = run.recv::<Report>() {
            break r;
        }
        run.wait();
    };
    println!("worker reports error {}", report.code);
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

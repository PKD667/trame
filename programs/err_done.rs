//! err_done: one worker runs two processes. `Finishes` (priority 0) returns Done at its first step. `Fails`
//! (priority 1) returns an error at its first step. Both have ended, so Fail answers `Err` with the error of
//! `Fails`. A `concurrent!` that answered `Ok` here would have ignored the error, because a process that
//! returned Done is not a process that is still running.

use trame::{Context, LEADER, Outcome, Status, Topology};

#[derive(Debug, PartialEq, Eq)]
struct Boom(u32);

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    code: u32,
}

struct Finishes;

#[trame::process]
impl Finishes {
    fn step(&mut self) -> Result<Status, Boom> {
        Ok(Status::Done)
    }
}

struct Fails;

#[trame::process]
impl Fails {
    fn step(&mut self) -> Result<Status, Boom> {
        Err(Boom(7))
    }
}

fn worker(_cx: &Context) {
    match trame::concurrent!(Finishes, Fails;) {
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

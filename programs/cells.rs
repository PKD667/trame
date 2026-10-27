//! cells: one worker runs a reader and a writer over one shared cell, `Tally`. The reader is declared
//! first, so it has priority 0. It waits on the cell (`Status::wait()`) until the writer has raised the tally
//! to 3. A step takes its cells before it starts and gives them back when it returns, so the reader's
//! step never runs inside a writer's step: each writer step ends, the reader wakes, and it checks again.
//! The worker logs each step to the leader, and the leader prints the log in arrival order.

use trame::{Context, Outcome, Status, Topology, LEADER};

const READER: u32 = 0;
const WRITER: u32 = 1;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Log {
    who: u32,
    n: u32,
}

/// The cell both processes share.
struct Tally {
    value: u32,
}

fn log(who: u32, n: u32) {
    assert!(trame::send(LEADER, Log { who, n }).is_ok(), "a log did not fit");
}

/// Priority 0: waits until the tally reaches 3, then logs it.
struct Reader;

#[trame::process]
impl Reader {
    fn step(&mut self, t: &mut Tally) -> Status {
        if t.value < 3 {
            return Status::wait();
        }
        log(READER, t.value);
        Status::Done
    }
}

/// Priority 1: three steps, each raising the tally by one.
struct Writer {
    n: u32,
}

#[trame::process]
impl Writer {
    fn step(&mut self, t: &mut Tally) -> Status {
        if self.n == 3 {
            return Status::Done;
        }
        self.n += 1;
        t.value += 1;
        log(WRITER, self.n);
        Status::Progress
    }
}

fn worker(_cx: &Context) {
    let mut tally = Tally { value: 0 };
    let r = trame::concurrent!(Reader, Writer { n: 0 }; tally);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(worker);
    t.route::<Log>(w, LEADER, 8);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let mut seen = 0;
    while seen < 4 {
        let mut got = false;
        while let Some(l) = run.recv::<Log>() {
            let who = if l.who == READER { "reader" } else { "writer" };
            println!("{who} {}", l.n);
            seen += 1;
            got = true;
        }
        if !got {
            run.wait();
        }
    }
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

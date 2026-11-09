//! cells: one worker whose state has a tally, read by `reader` and written by `writer`. The reader is
//! declared first, so it starts first; it waits (`Status::wait()`) until the tally reaches 3. A step
//! holds its fields from start to return, and the reader takes `value` as `&`, the writer as `&mut`,
//! so the reader never runs inside a writer step: each writer step ends, writes `value`, wakes the
//! reader, and the reader checks again. The worker logs each step to the leader, which prints the log
//! in arrival order.

use trame::{Leader, Outcome, Status, Topology};

const READER: u32 = 0;
const WRITER: u32 = 1;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Log {
    who: u32,
    n: u32,
}

/// The state: the shared tally, and the writer's count.
#[trame::state(steps(reader, writer))]
struct Tally {
    value: u32,
    n: u32,
}

fn log(who: u32, n: u32) {
    assert!(trame::send(Leader, Log { who, n }).is_ok(), "a log did not fit");
}

/// Waits until the tally reaches 3, then logs it.
#[trame::step(Tally)]
fn reader(value: &u32) -> Status {
    if *value < 3 {
        return Status::wait();
    }
    log(READER, *value);
    Status::Done
}

/// Three steps, each raising the tally by one.
#[trame::step(Tally)]
fn writer(value: &mut u32, n: &mut u32) -> Status {
    if *n == 3 {
        return Status::Done;
    }
    *n += 1;
    *value += 1;
    log(WRITER, *n);
    Status::Progress
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker();
    t.set(&w, move || Tally { value: 0, n: 0 });
    t.route::<Log>(w.id(), Leader, 8);
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

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

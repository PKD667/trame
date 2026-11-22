//! stamped: a worker sends three messages to the leader, and the leader receives each with the worker's clock at
//! the send (INTERFACE.md 5.1, 5.2). The three stamps are of one clock, so `since` answers a span, never `None`,
//! and taken both ways it gives the order: the later one's span is the difference, the earlier one's is zero.

use trame::{Leader, Outcome, Stamped, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Tick {
    n: u32,
}

#[trame::state(steps(tick))]
struct Ticker {
    next: u32,
}

#[trame::step(Ticker)]
fn tick(next: &mut u32) -> Status {
    if *next == 3 {
        return Status::Done;
    }
    match trame::send(Leader, Tick { n: *next }) {
        Ok(()) => {
            *next += 1;
            Status::Progress
        }
        Err(_) => Status::wait().room::<Tick>(Leader),
    }
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker::<Ticker>();
    t.set(&w, || Ticker { next: 0 });
    t.route::<Tick>(w.id(), Leader, 1);
    let run = t.launch().expect("accepted");
    let mut got: Vec<Stamped<Tick>> = Vec::new();
    while got.len() < 3 {
        match run.recv::<Tick>() {
            Some(s) => got.push(s),
            None => run.wait(),
        }
    }
    let order: Vec<u32> = got.iter().map(|s| s.value.n).collect();
    println!("order {order:?}");
    for (i, pair) in got.windows(2).enumerate() {
        let (a, b) = (pair[0].at, pair[1].at);
        assert_eq!(a.clock(), b.clock(), "one worker has one clock");
        let forward = b.since(a).expect("one clock");
        let backward = a.since(b).expect("one clock");
        assert_eq!(backward, core::time::Duration::ZERO, "the earlier stamp is not after the later one");
        println!("span {} {}", i + 1, if forward >= backward { "non-negative" } else { "negative" });
    }
    println!("outcomes: {:?}", run.join() == [Outcome::Returned]);
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

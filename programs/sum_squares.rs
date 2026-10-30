//! sum_squares: five workers each add the squares of their own range of 200000 integers and send the
//! partial sum to the leader, which prints the partials in worker order and their total. No worker talks
//! to another. The answer has a closed form, n(n-1)(2n-1)/6, so the expected output is not taken from a
//! run of either backend. Expected output: sum_squares.expected.

use trame::{Context, LEADER, Outcome, Status, Topology};

const WORKERS: u64 = 5;
const RANGE: u64 = 200_000;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Partial {
    worker: u64,
    sum: u64,
}

/// Adds k*k for k in [lo, hi), then reports once.
struct Adder {
    worker: u64,
    sum: Option<u64>,
}

#[trame::process]
impl Adder {
    fn step(&mut self) -> Status {
        let me = self.worker;
        let sum = *self.sum.get_or_insert_with(|| (me * RANGE..(me + 1) * RANGE).map(|k| k * k).sum());
        match trame::send(LEADER, Partial { worker: me, sum }) {
            Ok(()) => Status::Done,
            Err(_) => Status::wait().room::<Partial>(LEADER),
        }
    }
}

fn worker(cx: &Context) {
    let me = cx.workers().iter().position(|&w| w == cx.me()).expect("a worker is listed") as u64;
    assert_eq!(trame::concurrent!(Adder { worker: me, sum: None };), Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let ws: Vec<_> = (0..WORKERS).map(|_| t.worker(worker)).collect();
    for &w in &ws {
        t.route::<Partial>(w, LEADER, 1);
    }
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let mut parts: Vec<Option<u64>> = vec![None; WORKERS as usize];
    while parts.iter().any(Option::is_none) {
        match run.recv::<Partial>() {
            Some(p) => parts[p.worker as usize] = Some(p.sum),
            None => run.wait(),
        }
    }
    let mut total = 0u64;
    for (i, p) in parts.iter().enumerate() {
        let p = p.expect("every worker reported");
        total += p;
        println!("worker {i} partial {p}");
    }
    println!("total {total}");
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

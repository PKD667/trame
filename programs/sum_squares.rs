//! sum_squares: five workers each add the squares of their own range of 200000 integers and send the
//! partial sum to the leader, which prints the partials in worker order and their total. No worker talks
//! to another. The answer has a closed form, n(n-1)(2n-1)/6, so the expected output is not taken from a
//! run of either backend. Expected output: sum_squares.expected.

use trame::{Leader, Outcome, Status, Topology};

const WORKERS: u64 = 5;
const RANGE: u64 = 200_000;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Partial {
    worker: u64,
    sum: u64,
}

/// Worker `worker` adds k*k for k in its range once, then reports.
#[trame::state(steps(add))]
struct Adder {
    worker: u64,
    sum: Option<u64>,
}

#[trame::step(Adder)]
fn add(worker: &u64, sum: &mut Option<u64>) -> Status {
    let s = match *sum {
        Some(s) => s,
        None => {
            let mut s = 0u64;
            for k in *worker * RANGE..(*worker + 1) * RANGE {
                s += k * k;
            }
            *sum = Some(s);
            s
        }
    };
    match trame::send(Leader, Partial { worker: *worker, sum: s }) {
        Ok(()) => Status::Done,
        Err(_) => Status::wait().room::<Partial>(Leader),
    }
}

fn main() {
    let mut t = Topology::new();
    for i in 0..WORKERS {
        let w = t.worker();
        t.set(&w, move || Adder { worker: i, sum: None });
        t.route::<Partial>(w.id(), Leader, 1);
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

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

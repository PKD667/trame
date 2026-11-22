//! heap_in_state: a state owns a `Vec` that was made before its builder and moved in, so its block is on the heap of
//! the process that built the state. Where processes share a domain that is a launch error, named at the builder,
//! not a crash of another process at first use. A backend with one process has no such domain and runs it (its
//! skip.txt). The sibling `heap_cloned` clones inside the builder.

use trame::{Leader, Outcome, Status, Topology};

const WORKERS: u32 = 3;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Sum {
    worker: u64,
    sum: u64,
}

#[trame::state(steps(add))]
struct Adder {
    me: u32,
    list: Vec<u64>,
    sent: bool,
}

#[trame::step(Adder)]
fn add(me: &u32, list: &Vec<u64>, sent: &mut bool) -> Status {
    if *sent {
        return Status::Done;
    }
    let sum: u64 = list.iter().sum();
    assert!(trame::send(Leader, Sum { worker: u64::from(*me), sum }).is_ok(), "a sum did not fit");
    *sent = true;
    Status::Progress
}

fn main() {
    let made_outside: Vec<u64> = (1..=1000).collect();
    let mut t = Topology::new();
    let ws: Vec<_> = (0..WORKERS)
        .map(|me| {
            let w = t.worker();
            let outside = made_outside.clone();
            // `outside` is a heap block made here, before the builder, and moved into the state.
            t.set(&w, move || Adder { me, list: outside, sent: false });
            w
        })
        .collect();
    for w in &ws {
        t.route::<Sum>(w.id(), Leader, 2);
    }
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let mut sums = [0u64; WORKERS as usize];
    let mut n = 0;
    while n < WORKERS {
        let mut got = false;
        while let Some(s) = run.recv::<Sum>().map(|s| s.value) {
            sums[s.worker as usize] = s.sum;
            n += 1;
            got = true;
        }
        if !got {
            run.wait();
        }
    }
    for (w, s) in sums.iter().enumerate() {
        println!("worker {w} sum {s}");
    }
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

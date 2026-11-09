//! pool_edit: three workers and ten rounds. In round r the leader edits the pool (`Run::edit`): it sets
//! the round to r and pushes r onto a list, which allocates. Then it sends `Go(r)` to every worker. A
//! worker's step takes the `Go`, reads the pool (`pool: &Round`) and reports to the leader the round and
//! the list's length and sum. The leader prints each round's reports in worker order, and does not edit
//! for round r + 1 until all three reports for round r have arrived.
//!
//! A pool edit runs while no step that reads the pool runs, and a step that starts after the edit
//! returns reads the edited pool (CONTRACT.md, P2). The step that takes `Go(r)` starts after the leader
//! sent it, so after the edit: each worker reports exactly round r, a list of length r and sum
//! r(r+1)/2, on any backend.

use trame::{Leader, Outcome, Status, Topology};

const WORKERS: u32 = 3;
const ROUNDS: u32 = 10;

/// The pool: the round the leader last edited in, and every round so far.
#[derive(trame::Plain)]
struct Round {
    round: u32,
    all: Vec<u32>,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Go {
    round: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u32,
    round: u32,
    seen: u32,
    len: u32,
    sum: u32,
}

/// One worker: its index, and how many rounds it has reported.
#[trame::state(steps(probe), pool = Round)]
struct Probe {
    me: u32,
    done: u32,
}

/// Takes the `Go` of the next round and reports what the pool says.
#[trame::step(Probe, receives = Go)]
fn probe(me: &u32, done: &mut u32, pool: &Round) -> Status {
    if *done == ROUNDS {
        return Status::Done;
    }
    let Some(g) = trame::recv::<Go>() else {
        return Status::wait().frame();
    };
    assert_eq!(g.round, *done + 1, "a probe took the Go of another round");
    let rep = Report {
        worker: *me,
        round: g.round,
        seen: pool.round,
        len: pool.all.len() as u32,
        sum: pool.all.iter().sum(),
    };
    assert!(trame::send(Leader, rep).is_ok(), "a report did not fit");
    *done += 1;
    Status::Progress
}

fn main() {
    let mut t = Topology::new();
    let ws: Vec<_> = (0..WORKERS)
        .map(|me| {
            let w = t.worker();
            t.set(&w, move || Probe { me, done: 0 });
            w
        })
        .collect();
    let pool = t.pool();
    t.set(&pool, || Round { round: 0, all: Vec::new() });
    for w in &ws {
        t.route::<Go>(Leader, w.id(), 2);
        t.route::<Report>(w.id(), Leader, 2);
    }
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    for r in 1..=ROUNDS {
        assert!(run.edit(&pool, |p| {
            p.round = r;
            p.all.push(r);
        }));
        for w in &ws {
            assert!(run.send(w.id(), Go { round: r }).is_ok(), "a go did not fit");
        }
        let mut seen: Vec<Option<(u32, u32, u32)>> = vec![None; WORKERS as usize];
        let mut n = 0;
        while n < WORKERS {
            let mut got = false;
            while let Some(rep) = run.recv::<Report>() {
                assert_eq!(rep.round, r, "a report of another round");
                seen[rep.worker as usize] = Some((rep.seen, rep.len, rep.sum));
                n += 1;
                got = true;
            }
            if !got {
                run.wait();
            }
        }
        let line: Vec<String> = seen
            .iter()
            .enumerate()
            .map(|(w, s)| {
                let (seen, len, sum) = s.unwrap();
                format!("worker {w} saw {seen} len {len} sum {sum}")
            })
            .collect();
        println!("round {r}: {}", line.join(", "));
    }
    println!("edited {ROUNDS} rounds");
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

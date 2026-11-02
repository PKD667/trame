//! pool_publish: three workers and ten rounds. In round r the leader publishes the pool `Round(r)` with
//! `Run::publish`, then sends `Go(r)` to every worker. A worker's step takes the `Go`, reads the pool
//! (`pool: &Round`) and reports to the leader which round the pool named. The leader prints each round's
//! reports in worker order, and does not publish round r + 1 until all three reports for round r have
//! arrived.
//!
//! A step reads the pool version current at its start (P1), and a step that starts after `publish`
//! returns reads the new one (P2). That alone does not order the pool with frames: a step may start
//! before publish r and take `Go(r)`, sent after it, in the same run. So the step keeps the round it
//! was told in its state, and while the pool it pinned is older it returns `Progress`, so that its next
//! start pins again. Each worker then reports exactly `Round(r)` at round r, on any backend.

use trame::{LEADER, Outcome, Status, Topology};

const WORKERS: u32 = 3;
const ROUNDS: u32 = 10;

/// The host's pool: the round the leader last published.
struct Round(u32);

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
}

/// One worker: its index, how many rounds it has reported, and the round of a `Go` it took but has not
/// reported yet.
#[trame::state(steps(probe), pool = Round)]
struct Probe {
    me: u32,
    done: u32,
    told: Option<u32>,
}

/// Takes the `Go` of the next round, and reports what the pool says once it is at least that round.
#[trame::step(Probe, receives = Go)]
fn probe(me: &u32, done: &mut u32, told: &mut Option<u32>, pool: &Round) -> Status {
    if *done == ROUNDS {
        return Status::Done;
    }
    if told.is_none() {
        match trame::recv::<Go>() {
            Some(g) => {
                assert_eq!(g.round, *done + 1, "a probe took the Go of another round");
                *told = Some(g.round);
            }
            None => return Status::wait().frame(),
        }
    }
    let round = told.expect("set above");
    if pool.0 < round {
        // Pinned before the publish: start again, and pin the current version.
        return Status::Progress;
    }
    let sent = trame::send(LEADER, Report { worker: *me, round, seen: pool.0 });
    assert!(sent.is_ok(), "a report did not fit");
    *done += 1;
    *told = None;
    Status::Progress
}

fn main() {
    let mut t = Topology::new();
    let ws: Vec<_> = (0..WORKERS).map(|me| { let w = t.worker(); t.set(&w, move || Probe { me, done: 0, told: None }); w }).collect();
    t.pool(Round(0));
    for w in &ws {
        t.route::<Go>(LEADER, w.id(), 2);
        t.route::<Report>(w.id(), LEADER, 2);
    }
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    for r in 1..=ROUNDS {
        run.publish(Round(r));
        for w in &ws {
            assert!(run.send(w.id(), Go { round: r }).is_ok(), "a go did not fit");
        }
        let mut seen: Vec<Option<u32>> = vec![None; WORKERS as usize];
        let mut n = 0;
        while n < WORKERS {
            let mut got = false;
            while let Some(rep) = run.recv::<Report>() {
                assert_eq!(rep.round, r, "a report of another round");
                seen[rep.worker as usize] = Some(rep.seen);
                n += 1;
                got = true;
            }
            if !got {
                run.wait();
            }
        }
        let line: Vec<String> =
            seen.iter().enumerate().map(|(w, s)| format!("worker {w} saw {}", s.unwrap())).collect();
        println!("round {r}: {}", line.join(", "));
    }
    println!("published {ROUNDS} rounds");
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

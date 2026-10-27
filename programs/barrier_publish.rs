//! barrier_publish: three workers and ten rounds. In round r the leader publishes "round r", then sends
//! `Go(r)` to every worker. A worker passes its gate (a `concurrent!` whose process waits for `Go`),
//! calls the barrier, and reports what the barrier answered. The leader prints each round's reports
//! in worker order, and does not publish round r + 1 until all three reports for round r have arrived.
//! The order is forced: barrier r cannot fire before every gate passed, and every gate waits for a
//! `Go` sent after publish r. So each worker sees exactly "round r" at barrier r, on either backend.

use trame::{Context, Outcome, Status, Topology, LEADER};

const WORKERS: u32 = 3;
const ROUNDS: u32 = 10;

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

/// The cell the process shares. It holds nothing.
struct Shared;

/// Passes once `Go` for its round has arrived.
struct Gate {
    round: u32,
}

#[trame::process(receives = Go)]
impl Gate {
    fn step(&mut self, _s: &mut Shared) -> Status {
        match trame::recv::<Go>() {
            Some(g) => {
                assert_eq!(g.round, self.round, "a gate took the Go of another round");
                Status::Done
            }
            None => Status::wait().frame(),
        }
    }
}

/// The round a publication names: "round r" answers r.
fn round_of(bytes: &[u8]) -> u32 {
    let text = std::str::from_utf8(bytes).expect("a publication is text");
    text.trim_start_matches("round ").parse().expect("a publication names a round")
}

fn worker(cx: &Context) {
    // A worker's index is its position in the declaration order that `workers` lists.
    let me = cx.workers().iter().position(|&w| w == cx.me()).expect("a worker is listed") as u32;
    for r in 1..=ROUNDS {
        let mut shared = Shared;
        let passed = trame::concurrent!(Gate { round: r }; shared);
        assert_eq!(passed, Ok(()));
        let publication = trame::barrier();
        let seen = round_of(&publication);
        let sent = trame::send(LEADER, Report { worker: me, round: r, seen });
        assert!(sent.is_ok(), "a report did not fit");
    }
}

fn main() {
    let mut t = Topology::new();
    let ws: Vec<_> = (0..WORKERS).map(|_| t.worker(worker)).collect();
    t.publish(b"round 0");
    for &w in &ws {
        t.route::<Go>(LEADER, w, 2);
        t.route::<Report>(w, LEADER, 2);
    }
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    for r in 1..=ROUNDS {
        run.publish(format!("round {r}").as_bytes());
        for &w in &ws {
            assert!(run.send(w, Go { round: r }).is_ok(), "a go did not fit");
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
        let line: Vec<String> = seen.iter().enumerate().map(|(w, s)| format!("worker {w} saw {}", s.unwrap())).collect();
        println!("round {r}: {}", line.join(", "));
    }
    println!("published {ROUNDS} rounds");
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

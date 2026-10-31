//! wait_frame: a step waits with `Status::wait().frame()` for a frame of its own type, `Want`. The leader sends a
//! `Noise` frame (a type the step does not receive) and then a `Want`. The step must be called exactly
//! twice: once to say `Ready` and wait, and once when `Want` arrives. A call for `Noise` would show as a
//! third call. The leader sends nothing until `Ready`, so the first call always comes before `Want`.

use trame::{LEADER, Outcome, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Ready;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Noise;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Want;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Calls {
    n: u32,
}

/// The state: the call count of `picky`, and whether it said `Ready`.
#[trame::state(steps(picky))]
struct Picky {
    calls: u32,
    said_ready: bool,
}

/// Counts its calls. Receives `Want` only.
#[trame::step(Picky, receives = Want)]
fn picky(calls: &mut u32, said_ready: &mut bool) -> Status {
    *calls += 1;
    if !*said_ready {
        *said_ready = true;
        assert!(trame::send(LEADER, Ready).is_ok(), "ready did not fit");
        return Status::wait().frame();
    }
    match trame::recv::<Want>() {
        Some(Want) => {
            assert!(trame::send(LEADER, Calls { n: *calls }).is_ok(), "calls did not fit");
            Status::Done
        }
        None => Status::wait().frame(),
    }
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(Picky { calls: 0, said_ready: false }).id();
    t.route::<Ready>(w, LEADER, 2);
    t.route::<Calls>(w, LEADER, 2);
    t.route::<Noise>(LEADER, w, 2);
    t.route::<Want>(LEADER, w, 2);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    while run.recv::<Ready>().is_none() {
        run.wait();
    }
    assert!(run.send(w, Noise).is_ok(), "noise did not fit");
    assert!(run.send(w, Want).is_ok(), "want did not fit");
    let calls = loop {
        if let Some(c) = run.recv::<Calls>() {
            break c;
        }
        run.wait();
    };
    println!("process called {} times", calls.n);
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

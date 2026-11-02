//! frame_other: two steps in one worker. `picky` (declared first) receives `Want` and waits for a frame.
//! `taker` (declared second) receives `Noise` and waits for a frame. The leader sends `Noise`, waits
//! until `taker` reports it took the frame, then sends `Want`. `picky` is woken only by a frame it
//! receives, so it is called once for `Ready` and once for `Want`: it reports n = 2. A step woken by a
//! frame of another step would be called a third time before `Want`, and report n = 3.

use trame::{LEADER, Outcome, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Ready;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Noise;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Took;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Want;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Calls {
    n: u32,
}

/// The state: `picky`'s call count and whether it said `Ready`. `taker` uses no field.
#[trame::state(steps(picky, taker))]
struct Pair {
    calls: u32,
    said_ready: bool,
}

#[trame::step(Pair, receives = Want)]
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

#[trame::step(Pair, receives = Noise)]
fn taker() -> Status {
    match trame::recv::<Noise>() {
        Some(Noise) => {
            assert!(trame::send(LEADER, Took).is_ok(), "took did not fit");
            Status::Done
        }
        None => Status::wait().frame(),
    }
}

fn main() {
    let mut t = Topology::new();
    let w_worker = t.worker();
    t.set(&w_worker, || Pair { calls: 0, said_ready: false });
    let w = w_worker.id();
    t.route::<Ready>(w, LEADER, 2);
    t.route::<Took>(w, LEADER, 2);
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
    while run.recv::<Took>().is_none() {
        run.wait();
    }
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

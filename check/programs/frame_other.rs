//! frame_other: two steps in one worker. `picky` (declared first) receives `Want` and waits for a frame.
//! `taker` (declared second) receives `Noise` and waits for a frame. Each says hello to the leader and then
//! waits, and the leader sends `Noise` only after both hellos, so both are waiting when `Noise` is queued. The
//! leader then waits until `taker` reports it took the frame, and sends `Want`. `picky` is woken only by a frame
//! it receives, so it is called once for `Ready` and once for `Want`: it reports n = 2. A step woken by another
//! step's frame is enabled while `Noise` is queued. Both steps have weight 0, so the least recently started goes
//! first, and `picky`, which started first, runs once more before `taker` takes `Noise`: it reports n = 3.

use trame::{Leader, Outcome, Status, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Hello;

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

/// The state: `picky`'s call count, whether it has said `Ready`, and whether `taker` has said `Hello`.
#[trame::state(steps(picky, taker))]
struct Pair {
    calls: u32,
    said_ready: bool,
    said_hello: bool,
}

#[trame::step(Pair, receives = Want)]
fn picky(calls: &mut u32, said_ready: &mut bool) -> Status {
    *calls += 1;
    if !*said_ready {
        *said_ready = true;
        assert!(trame::send(Leader, Ready).is_ok(), "ready did not fit");
        return Status::wait().frame();
    }
    match trame::recv::<Want>() {
        Some(Want) => {
            assert!(trame::send(Leader, Calls { n: *calls }).is_ok(), "calls did not fit");
            Status::Done
        }
        None => Status::wait().frame(),
    }
}

#[trame::step(Pair, receives = Noise)]
fn taker(said_hello: &mut bool) -> Status {
    if !*said_hello {
        *said_hello = true;
        assert!(trame::send(Leader, Hello).is_ok(), "hello did not fit");
        return Status::wait().frame();
    }
    match trame::recv::<Noise>() {
        Some(Noise) => {
            assert!(trame::send(Leader, Took).is_ok(), "took did not fit");
            Status::Done
        }
        None => Status::wait().frame(),
    }
}

fn main() {
    let mut t = Topology::new();
    let w_worker = t.worker();
    t.set(&w_worker, || Pair { calls: 0, said_ready: false, said_hello: false });
    let w = w_worker.id();
    t.route::<Hello>(w, Leader, 2);
    t.route::<Ready>(w, Leader, 2);
    t.route::<Took>(w, Leader, 2);
    t.route::<Calls>(w, Leader, 2);
    t.route::<Noise>(Leader, w, 2);
    t.route::<Want>(Leader, w, 2);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    // Both steps must be waiting before Noise is sent: `picky` says Ready, `taker` says Hello.
    while run.recv::<Ready>().is_none() {
        run.wait();
    }
    while run.recv::<Hello>().is_none() {
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

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

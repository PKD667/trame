//! frame_other: two processes in one worker. `Picky` (priority 0) owns `Want` and waits for a frame. `Taker`
//! (priority 1) owns `Noise` and waits for a frame. The leader sends `Noise`, waits until `Taker` reports it
//! took the frame, then sends `Want`. `Picky` is enabled only by a frame it owns (spec.md 5.3, Enabled), so it
//! is called once for `Ready` and once for `Want`: it reports n = 2. A process woken by a frame of another
//! process would be called a third time before `Want`, and report n = 3.

use trame::{Context, LEADER, Outcome, Status, Topology};

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

struct Picky {
    calls: u32,
    said_ready: bool,
}

#[trame::process(receives = Want)]
impl Picky {
    fn step(&mut self) -> Status {
        self.calls += 1;
        if !self.said_ready {
            self.said_ready = true;
            assert!(trame::send(LEADER, Ready).is_ok(), "ready did not fit");
            return Status::wait().frame();
        }
        match trame::recv::<Want>() {
            Some(Want) => {
                assert!(trame::send(LEADER, Calls { n: self.calls }).is_ok(), "calls did not fit");
                Status::Done
            }
            None => Status::wait().frame(),
        }
    }
}

struct Taker;

#[trame::process(receives = Noise)]
impl Taker {
    fn step(&mut self) -> Status {
        match trame::recv::<Noise>() {
            Some(Noise) => {
                assert!(trame::send(LEADER, Took).is_ok(), "took did not fit");
                Status::Done
            }
            None => Status::wait().frame(),
        }
    }
}

fn worker(_cx: &Context) {
    let r = trame::concurrent!(Picky { calls: 0, said_ready: false }, Taker;);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(worker);
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

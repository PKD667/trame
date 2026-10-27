//! wait_frame: a process waits with `Status::wait().frame()` for a frame of its own type, `Want`. The leader sends a
//! `Noise` frame (a type the process does not own) and then a `Want`. The process must be called exactly
//! twice: once to say `Ready` and wait, and once when `Want` arrives. A call for `Noise` would show as a
//! third call. The leader sends nothing until `Ready`, so the first call always comes before `Want`.

use trame::{Context, Outcome, Status, Topology, LEADER};

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

/// Counts its calls. Owns `Want` only.
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

fn worker(_cx: &Context) {
    let r = trame::concurrent!(Picky { calls: 0, said_ready: false };);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(worker);
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

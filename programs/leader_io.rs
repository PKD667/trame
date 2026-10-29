//! leader_io: the leader sends three commands to each of two workers. Each worker answers every
//! command on its own route, with its own message type, so the leader's output per worker is in the
//! order that worker sent. The leader prints the answers, then the outcomes.

use trame::{Context, LEADER, Outcome, Status, Topology};

const N: u32 = 3;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Cmd {
    v: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Reply0 {
    v: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Reply1 {
    v: u32,
}

/// The cell the processes share. It holds nothing: the processes talk over routes.
struct Shared;

/// Worker 0 answers each command with `10 * v`.
struct Echo0 {
    got: u32,
    pending: Option<u32>,
}

#[trame::process(receives = Cmd)]
impl Echo0 {
    fn step(&mut self, _s: &mut Shared) -> Status {
        if let Some(v) = self.pending {
            return match trame::send(LEADER, Reply0 { v }) {
                Ok(()) => {
                    self.pending = None;
                    Status::Progress
                }
                Err(_) => Status::wait().room::<Reply0>(LEADER),
            };
        }
        if self.got == N {
            return Status::Done;
        }
        match trame::recv::<Cmd>() {
            Some(c) => {
                self.got += 1;
                self.pending = Some(c.v * 10);
                Status::Progress
            }
            None => Status::wait().frame(),
        }
    }
}

/// Worker 1 answers each command with `v + 100`.
struct Echo1 {
    got: u32,
    pending: Option<u32>,
}

#[trame::process(receives = Cmd)]
impl Echo1 {
    fn step(&mut self, _s: &mut Shared) -> Status {
        if let Some(v) = self.pending {
            return match trame::send(LEADER, Reply1 { v }) {
                Ok(()) => {
                    self.pending = None;
                    Status::Progress
                }
                Err(_) => Status::wait().room::<Reply1>(LEADER),
            };
        }
        if self.got == N {
            return Status::Done;
        }
        match trame::recv::<Cmd>() {
            Some(c) => {
                self.got += 1;
                self.pending = Some(c.v + 100);
                Status::Progress
            }
            None => Status::wait().frame(),
        }
    }
}

fn worker0(_cx: &Context) {
    let mut shared = Shared;
    let r = trame::concurrent!(Echo0 { got: 0, pending: None }; shared);
    assert_eq!(r, Ok(()));
}

fn worker1(_cx: &Context) {
    let mut shared = Shared;
    let r = trame::concurrent!(Echo1 { got: 0, pending: None }; shared);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let a = t.worker(worker0);
    let b = t.worker(worker1);
    t.route::<Cmd>(LEADER, a, 4);
    t.route::<Cmd>(LEADER, b, 4);
    t.route::<Reply0>(a, LEADER, 4);
    t.route::<Reply1>(b, LEADER, 4);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    for v in 0..N {
        assert!(run.send(a, Cmd { v }).is_ok(), "a command did not fit");
        assert!(run.send(b, Cmd { v }).is_ok(), "a command did not fit");
    }
    let (mut r0, mut r1) = (Vec::new(), Vec::new());
    while r0.len() < N as usize || r1.len() < N as usize {
        let mut got = false;
        if let Some(r) = run.recv::<Reply0>() {
            r0.push(r.v);
            got = true;
        }
        if let Some(r) = run.recv::<Reply1>() {
            r1.push(r.v);
            got = true;
        }
        if !got {
            run.wait();
        }
    }
    println!("reply0 {r0:?}");
    println!("reply1 {r1:?}");
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

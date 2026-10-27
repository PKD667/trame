//! lossy: worker 0 sends ten frames on a lossy route of capacity 4, which never answers Full, then a
//! lossless "done" token. Worker 1 waits for the token before it reads the lossy route, so every
//! overwrite has already happened and the output does not depend on timing: the four newest frames
//! survive. Worker 1 reports them to the leader, which prints them and the outcomes.

use trame::{Context, Outcome, Status, Topology, WorkerId, LEADER};

const SENT: u32 = 10;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Lossy {
    n: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Done {
    token: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Ev {
    v: u32,
}

/// The cell the process shares. It holds nothing.
struct Shared;

/// Worker 1: waits for the token, then reports every lossy frame that is still there.
struct Drain {
    token: bool,
    pending: Option<u32>,
    to: WorkerId,
}

#[trame::process(receives = Done, Lossy)]
impl Drain {
    fn step(&mut self, _s: &mut Shared) -> Status {
        if let Some(v) = self.pending {
            return match trame::send(self.to, Ev { v }) {
                Ok(()) => {
                    self.pending = None;
                    Status::Progress
                }
                Err(_) => Status::wait().room::<Ev>(self.to),
            };
        }
        if !self.token {
            return match trame::recv::<Done>() {
                Some(_) => {
                    self.token = true;
                    Status::Progress
                }
                None => Status::wait().frame(),
            };
        }
        match trame::recv::<Lossy>() {
            Some(l) => {
                self.pending = Some(l.n);
                Status::Progress
            }
            None => Status::Done,
        }
    }
}

fn sender(cx: &Context) {
    let to = cx.workers()[1];
    for n in 0..SENT {
        assert!(trame::send(to, Lossy { n }).is_ok(), "a lossy send answered Full");
    }
    assert!(trame::send(to, Done { token: 1 }).is_ok(), "the token did not fit");
}

fn receiver(_cx: &Context) {
    let mut shared = Shared;
    let r = trame::concurrent!(Drain { token: false, pending: None, to: LEADER }; shared);
    assert_eq!(r, Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let a = t.worker(sender);
    let b = t.worker(receiver);
    t.lossy::<Lossy>(a, b, 4);
    t.route::<Done>(a, b, 1);
    t.route::<Ev>(b, LEADER, 8);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let mut kept = Vec::new();
    while kept.len() < 4 {
        match run.recv::<Ev>() {
            Some(e) => kept.push(e.v),
            None => run.wait(),
        }
    }
    for v in kept {
        println!("kept {v}");
    }
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

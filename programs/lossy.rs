//! lossy: worker 0 sends ten frames on a lossy route of capacity 4, which never answers Full, then a
//! lossless "done" token, all in one step. Worker 1 waits for the token before it reads the lossy route, so
//! every overwrite has already happened and the output does not depend on timing: the four newest frames
//! survive. Worker 1 reports them to the leader, which prints them and the outcomes.

use trame::{LEADER, Outcome, Status, Topology, WorkerId};

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

/// Worker 0: `to` is worker 1.
#[trame::state(steps(sender))]
struct Sender {
    to: WorkerId,
}

#[trame::step(Sender)]
fn sender(to: &WorkerId) -> Status {
    for n in 0..SENT {
        assert!(trame::send(*to, Lossy { n }).is_ok(), "a lossy send answered Full");
    }
    assert!(trame::send(*to, Done { token: 1 }).is_ok(), "the token did not fit");
    Status::Done
}

/// Worker 1: whether the token came, and the report it still has to send.
#[trame::state(steps(drain))]
struct Drain {
    token: bool,
    pending: Option<u32>,
}

/// Waits for the token, then reports every lossy frame that is still there.
#[trame::step(Drain, receives = Done, Lossy)]
fn drain(token: &mut bool, pending: &mut Option<u32>) -> Status {
    if let Some(v) = *pending {
        return match trame::send(LEADER, Ev { v }) {
            Ok(()) => {
                *pending = None;
                Status::Progress
            }
            Err(_) => Status::wait().room::<Ev>(LEADER),
        };
    }
    if !*token {
        return match trame::recv::<Done>() {
            Some(_) => {
                *token = true;
                Status::Progress
            }
            None => Status::wait().frame(),
        };
    }
    match trame::recv::<Lossy>() {
        Some(l) => {
            *pending = Some(l.n);
            Status::Progress
        }
        None => Status::Done,
    }
}

fn main() {
    let mut t = Topology::new();
    let a = t.worker();
    let b = t.worker();
    let to = b.id();
    t.set(&a, move || Sender { to });
    t.set(&b, || Drain { token: false, pending: None });
    t.lossy::<Lossy>(a.id(), b.id(), 4);
    t.route::<Done>(a.id(), b.id(), 1);
    t.route::<Ev>(b.id(), LEADER, 8);
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

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

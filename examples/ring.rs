//! ring: W workers pass a token around a ring for ROUNDS rounds, then report to the leader.
use trame::{LEADER, Status, Topology, Worker, WorkerId};

const W: usize = 4;
const ROUNDS: u32 = 3;

#[trame::message]
struct Token {
    value: u32,
}

#[trame::message]
struct Report {
    worker: u32,
    total: u32,
}

#[trame::state(steps(hop))]
struct Hop {
    me: u32,
    next: WorkerId,
    round: u32,
    value: u32,
    total: u32,
    sent: bool,
}

#[trame::step(Hop, receives = Token)]
fn hop(me: &u32, next: &WorkerId, round: &mut u32, value: &mut u32, total: &mut u32, sent: &mut bool) -> Status {
    if *round == ROUNDS {
        return match trame::send(LEADER, Report { worker: *me, total: *total }) {
            Ok(()) => Status::Done,
            Err(_) => Status::wait().room::<Report>(LEADER),
        };
    }
    if !*sent {
        if trame::send(*next, Token { value: *value }).is_err() {
            return Status::wait().room::<Token>(*next);
        }
        *sent = true;
    }
    match trame::recv::<Token>() {
        Some(t) => {
            (*value, *total, *round, *sent) = (t.value, *total + t.value, *round + 1, false);
            Status::Progress
        }
        None => Status::wait().frame(),
    }
}

fn main() {
    let mut topo = Topology::new();
    let ws: Vec<Worker<Hop>> = (0..W).map(|_| topo.worker()).collect();
    for i in 0..W {
        let next = ws[(i + 1) % W].id();
        topo.set(&ws[i], move || Hop { me: i as u32, next, round: 0, value: i as u32 + 1, total: 0, sent: false });
        topo.route::<Token>(ws[i].id(), next, 1);
        topo.route::<Report>(ws[i].id(), LEADER, 1);
    }
    let run = topo.launch().expect("the ring launches");
    let mut totals = [None; W];
    while totals.iter().any(Option::is_none) {
        match run.recv::<Report>() {
            Some(r) => totals[r.worker as usize] = Some(r.total),
            None => run.wait(),
        }
    }
    for (i, t) in totals.iter().enumerate() {
        println!("worker {i} total {}", t.expect("every worker reported"));
    }
    println!("outcomes: {:?}", run.join());
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

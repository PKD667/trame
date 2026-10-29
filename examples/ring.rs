//! ring: W workers pass a token around a ring for ROUNDS rounds, then report to the leader.
use trame::{Context, LEADER, Status, Topology, WorkerId};

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

struct Hop {
    me: u32,
    next: WorkerId,
    round: u32,
    value: u32,
    total: u32,
    sent: bool,
}

#[trame::process(receives = Token)]
impl Hop {
    fn step(&mut self) -> Status {
        if self.round == ROUNDS {
            let report = Report { worker: self.me, total: self.total };
            return match trame::send(LEADER, report) {
                Ok(()) => Status::Done,
                Err(_) => Status::wait().room::<Report>(LEADER),
            };
        }
        if !self.sent {
            if trame::send(self.next, Token { value: self.value }).is_err() {
                return Status::wait().room::<Token>(self.next);
            }
            self.sent = true;
        }
        match trame::recv::<Token>() {
            Some(t) => {
                (self.value, self.total, self.round, self.sent) =
                    (t.value, self.total + t.value, self.round + 1, false);
                Status::Progress
            }
            None => Status::wait().frame(),
        }
    }
}

fn worker(cx: &Context) {
    let me = cx.workers().iter().position(|&w| w == cx.me()).expect("a worker is listed");
    let hop =
        Hop { me: me as u32, next: cx.workers()[(me + 1) % W], round: 0, value: me as u32 + 1, total: 0, sent: false };
    assert_eq!(trame::concurrent!(hop;), Ok(()));
}

fn main() {
    let mut topo = Topology::new();
    let ws: Vec<WorkerId> = (0..W).map(|_| topo.worker(worker)).collect();
    for i in 0..W {
        topo.route::<Token>(ws[i], ws[(i + 1) % W], 1);
        topo.route::<Report>(ws[i], LEADER, 1);
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

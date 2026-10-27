//! scale_fanin W MSGS: every worker sends MSGS `Small` messages (64 bytes) to the leader on a route of
//! capacity 64, waiting for room when it is full. The leader is the only sink. Each worker reports the
//! span of its own sends on its own clock. The leader times its receives with `std::time::Instant`,
//! because `trame::now` is not available to the leader; that clock is outside the contract, and the
//! number is a host measurement. Built with `--cfg trame_checked`.

#[path = "common.rs"]
mod common;

use std::time::Instant;
use trame::{Context, Reading, Status, Topology, LEADER};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Small {
    seq: u64,
    from: u64,
    pad: [u8; 48],
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u64,
    ns: u64,
}

/// Sends `msgs` messages to the leader, as fast as the route allows, then reports.
struct Feeder {
    me: u64,
    msgs: u64,
    sent: u64,
    t0: Option<Reading>,
    ns: Option<u64>,
    reported: bool,
}

#[trame::process]
impl Feeder {
    fn step(&mut self) -> Status {
        let t0 = *self.t0.get_or_insert_with(trame::now);
        while self.sent < self.msgs {
            if trame::send(LEADER, Small { seq: self.sent, from: self.me, pad: [0; 48] }).is_err() {
                return Status::wait().room::<Small>(LEADER);
            }
            self.sent += 1;
        }
        let ns = *self.ns.get_or_insert_with(|| common::ns(trame::now().since(t0)));
        if self.reported {
            return Status::Done;
        }
        match trame::send(LEADER, Report { worker: self.me, ns }) {
            Ok(()) => {
                self.reported = true;
                Status::Progress
            }
            Err(_) => Status::wait().room::<Report>(LEADER),
        }
    }
}

fn worker(me: u64, msgs: u64) {
    let r = trame::concurrent!(Feeder { me, msgs, sent: 0, t0: None, ns: None, reported: false };);
    assert_eq!(r, Ok(()));
}

fn main() {
    let w: usize = common::arg(1, "W");
    let msgs: u64 = common::arg(2, "MSGS");
    let mut t = Topology::new();
    let mut ids = Vec::new();
    for i in 0..w {
        let id = t.worker(move |_cx: &Context| worker(i as u64, msgs));
        ids.push(id);
    }
    for id in &ids {
        t.route::<Small>(*id, LEADER, 64);
        t.route::<Report>(*id, LEADER, 1);
    }
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    let total = w as u64 * msgs;
    let (mut data, mut reports) = (0u64, 0usize);
    let start = Instant::now();
    let mut leader_ns = None;
    while data < total || reports < w {
        if run.recv::<Small>().is_some() {
            data += 1;
            if data == total {
                leader_ns = Some(u64::try_from(start.elapsed().as_nanos()).expect("a span over 584 years"));
            }
        } else if let Some(r) = run.recv::<Report>() {
            println!("{{\"bench\":\"fanin\",\"workers\":{w},\"worker\":{},\"msgs\":{msgs},\"ns\":{}}}", r.worker, r.ns);
            reports += 1;
        } else {
            run.wait();
        }
    }
    let ns = leader_ns.unwrap_or(0);
    println!("{{\"bench\":\"fanin\",\"workers\":{w},\"msgs\":{msgs},\"ns\":{ns}}}");
    common::finish(run);
}

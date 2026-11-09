//! bench_fanin W MSGS: every worker sends MSGS `Small` messages (64 bytes) to the leader on a route of
//! capacity 64, waiting for room when it is full. The leader is the only sink. Each worker reports the
//! span of its own sends on its own clock. The leader times its receives with `std::time::Instant`,
//! because `trame::now` is not available to the leader; that clock is outside the contract, and the
//! number is a host measurement. Built with `--cfg trame_checked`.

#[path = "common.rs"]
mod common;

use std::time::Instant;
use trame::{Leader, Reading, Status, Topology, Worker};

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

/// One worker: its index, how many messages to send, how many it has, and its span.
#[trame::state(steps(feed))]
struct Feeder {
    me: u64,
    msgs: u64,
    sent: u64,
    t0: Option<Reading>,
    ns: Option<u64>,
    reported: bool,
}

/// Sends `msgs` messages to the leader, as fast as the route allows, then reports.
#[trame::step(Feeder)]
fn feed(
    me: &u64,
    msgs: &u64,
    sent: &mut u64,
    t0: &mut Option<Reading>,
    ns: &mut Option<u64>,
    reported: &mut bool,
) -> Status {
    let start = *t0.get_or_insert_with(trame::now);
    while *sent < *msgs {
        if trame::send(Leader, Small { seq: *sent, from: *me, pad: [0; 48] }).is_err() {
            return Status::wait().room::<Small>(Leader);
        }
        *sent += 1;
    }
    let ns = *ns.get_or_insert_with(|| common::ns(trame::now().since(start)));
    if *reported {
        return Status::Done;
    }
    match trame::send(Leader, Report { worker: *me, ns }) {
        Ok(()) => {
            *reported = true;
            Status::Progress
        }
        Err(_) => Status::wait().room::<Report>(Leader),
    }
}

fn main() {
    let w: usize = common::arg(1, "W");
    let msgs: u64 = common::arg(2, "MSGS");
    let mut t = Topology::new();
    let ws: Vec<Worker<Feeder>> = (0..w).map(|_| t.worker()).collect();
    for (i, h) in ws.iter().enumerate() {
        t.set(h, move || Feeder { me: i as u64, msgs, sent: 0, t0: None, ns: None, reported: false });
        t.route::<Small>(h.id(), Leader, 64);
        t.route::<Report>(h.id(), Leader, 1);
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

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

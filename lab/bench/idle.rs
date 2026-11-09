//! bench_idle W SECONDS: W workers, each waiting with `.frame().until(deadline)` for SECONDS
//! on its own clock. Nothing is ever sent to a worker. Each worker reports the span it actually waited.
//! Its point is CPU time, which lab/bench.py measures per rank with GNU time. The leader prints one JSON
//! line per worker, then the outcomes. Built with `--cfg trame_checked`.

#[path = "common.rs"]
mod common;

use core::time::Duration;
use trame::{Leader, Reading, Status, Topology, Worker};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u64,
    ns: u64,
}

/// One worker: its index, how long it waits, and the span it measured and whether it reported.
#[trame::state(steps(idle))]
struct Idle {
    me: u64,
    span: Duration,
    start: Option<Reading>,
    ns: Option<u64>,
    reported: bool,
}

/// Waits for `span` from its first step, woken only by its deadline (no frame ever comes).
#[trame::step(Idle)]
fn idle(me: &u64, span: &Duration, start: &mut Option<Reading>, ns: &mut Option<u64>, reported: &mut bool) -> Status {
    let now = trame::now();
    let start = *start.get_or_insert(now);
    if now.since(start) < *span {
        return Status::wait().frame().until(start.after(*span));
    }
    let ns = *ns.get_or_insert(common::ns(now.since(start)));
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
    let seconds: u64 = common::arg(2, "SECONDS");
    let span = Duration::from_secs(seconds);
    let mut t = Topology::new();
    let ws: Vec<Worker<Idle>> = (0..w).map(|_| t.worker()).collect();
    for (i, h) in ws.iter().enumerate() {
        t.set(h, move || Idle { me: i as u64, span, start: None, ns: None, reported: false });
        t.route::<Report>(h.id(), Leader, 1);
    }
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    common::collect::<Report>(&run, w, |r| {
        println!(
            "{{\"bench\":\"idle\",\"workers\":{w},\"worker\":{},\"seconds\":{seconds},\"ns\":{}}}",
            r.worker, r.ns
        );
    });
    common::finish(run);
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

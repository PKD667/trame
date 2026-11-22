//! bench_idle W SECONDS: W workers, each waiting with `.frame()`; the leader sleeps SECONDS, then sends each a
//! `Go`. Each worker reports the span on its own clock between its first step and the `Go`.
//! Its point is CPU time, which lab/bench.py measures per rank with GNU time. The leader prints one JSON
//! line per worker, then the outcomes. Built with `--cfg trame_checked`.

#[path = "common.rs"]
mod common;

use trame::{Leader, Reading, Status, Topology, Worker};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Go {
    pad: u64,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u64,
    ns: u64,
}

/// One worker: its index, when it started, the span it measured and whether it reported.
#[trame::state(steps(idle))]
struct Idle {
    me: u64,
    start: Option<Reading>,
    ns: Option<u64>,
    reported: bool,
}

/// Waits for a frame, woken by nothing else: `Go` comes after the leader's sleep.
#[trame::step(Idle, receives = Go)]
fn idle(me: &u64, start: &mut Option<Reading>, ns: &mut Option<u64>, reported: &mut bool) -> Status {
    let now = trame::now();
    let start = *start.get_or_insert(now);
    if ns.is_none() {
        if trame::recv::<Go>().is_none() {
            return Status::wait().frame();
        }
        *ns = Some(common::ns(now.since(start).expect("one worker has one clock")));
    }
    let ns = ns.expect("set above");
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
    let mut t = Topology::new();
    let ws: Vec<Worker<Idle>> = (0..w).map(|_| t.worker()).collect();
    for (i, h) in ws.iter().enumerate() {
        t.set(h, move || Idle { me: i as u64, start: None, ns: None, reported: false });
        t.route::<Report>(h.id(), Leader, 1);
        t.route::<Go>(Leader, h.id(), 1);
    }
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    std::thread::sleep(core::time::Duration::from_secs(seconds));
    for h in &ws {
        run.send(h.id(), Go { pad: 0 }).expect("room for one Go");
    }
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

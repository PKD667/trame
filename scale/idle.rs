//! scale_idle W SECONDS: W workers, each waiting in `concurrent!` with `.frame().until(deadline)` for SECONDS
//! on its own clock. Nothing is ever sent to a worker. Each worker reports the span it actually waited.
//! Its point is CPU time, which scale/run.sh measures per rank with GNU time. The leader prints one JSON
//! line per worker, then the outcomes. Built with `--cfg trame_checked`.

#[path = "common.rs"]
mod common;

use core::time::Duration;
use trame::{Context, Reading, Status, Topology, LEADER};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u64,
    ns: u64,
}

/// Waits for `span` from its first step, woken only by its deadline (no frame ever comes).
struct Idle {
    me: u64,
    span: Duration,
    start: Option<Reading>,
    ns: Option<u64>,
    reported: bool,
}

#[trame::process]
impl Idle {
    fn step(&mut self) -> Status {
        let now = trame::now();
        let start = *self.start.get_or_insert(now);
        if now.since(start) < self.span {
            return Status::wait().frame().until(start.after(self.span));
        }
        let ns = *self.ns.get_or_insert(common::ns(now.since(start)));
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

fn worker(me: u64, span: Duration) {
    let r = trame::concurrent!(Idle { me, span, start: None, ns: None, reported: false };);
    assert_eq!(r, Ok(()));
}

fn main() {
    let w: usize = common::arg(1, "W");
    let seconds: u64 = common::arg(2, "SECONDS");
    let span = Duration::from_secs(seconds);
    let mut t = Topology::new();
    let mut ids = Vec::new();
    for i in 0..w {
        let id = t.worker(move |_cx: &Context| worker(i as u64, span));
        ids.push(id);
    }
    for id in &ids {
        t.route::<Report>(*id, LEADER, 1);
    }
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    common::collect::<Report>(&run, w, |r| {
        println!("{{\"bench\":\"idle\",\"workers\":{w},\"worker\":{},\"seconds\":{seconds},\"ns\":{}}}", r.worker, r.ns);
    });
    common::finish(run);
}

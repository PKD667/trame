//! scale_barrier W ROUNDS: W workers. After one warm-up barrier, each worker calls `trame::barrier()`
//! ROUNDS times, timed on its own clock, and sends one Report to the leader. The leader prints one JSON
//! line per worker, then the outcomes. Built with `--cfg trame_checked` (flake.nix sets it).

#[path = "common.rs"]
mod common;

use trame::{Context, Topology, LEADER};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u64,
    ns: u64,
}

fn main() {
    let w: usize = common::arg(1, "W");
    let rounds: u64 = common::arg(2, "ROUNDS");
    if w == 0 {
        eprintln!("usage: W must be at least 1");
        std::process::exit(2);
    }
    let mut t = Topology::new();
    let mut ids = Vec::new();
    for i in 0..w {
        let id = t.worker(move |_cx: &Context| {
            let _ = trame::barrier();
            let t0 = trame::now();
            for _ in 0..rounds {
                let _ = trame::barrier();
            }
            let ns = common::ns(trame::now().since(t0));
            trame::send(LEADER, Report { worker: i as u64, ns }).expect("the report did not fit");
        });
        ids.push(id);
    }
    for id in &ids {
        t.route::<Report>(*id, LEADER, 1);
    }
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    common::collect::<Report>(&run, w, |r| {
        println!("{{\"bench\":\"barrier\",\"workers\":{w},\"worker\":{},\"rounds\":{rounds},\"ns\":{}}}", r.worker, r.ns);
    });
    common::finish(run);
}

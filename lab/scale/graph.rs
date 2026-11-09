//! scale_graph W STEPS SIZE DEGREE ITERS SEED: W workers on the circulant graph of graph_net.rs. Worker w has a
//! lossless route of capacity CAP to (w+o) mod W for each offset o, and receives from (w-o) mod W. Each step: ITERS
//! xorshift rounds, one SIZE-byte frame to each out-neighbour, one from each in-neighbour. Payload: bytes 0..8 the
//! step, 8..16 the sender. Warm-up 10 steps; each worker times STEPS steps and its compute part on its own clock.
//! Checksum: the xorshift state XOR the sum over all frames received of ((sender+1) << 32 | (step+1)), so it
//! covers the graph and every frame, not only the recurrence.
//! The leader prints one JSON line per worker, then the outcomes. Built with `--cfg trame_checked`.

#[path = "common.rs"]
mod common;
#[path = "graph_net.rs"]
mod graph_net;
#[macro_use]
#[path = "graph_proc.rs"]
mod graph_proc;

use trame::{Leader, Reading, Status, Topology, Worker, WorkerId};

const WARM: u64 = 10;
const CAP: usize = 4;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u64,
    ns: u64,
    compute_ns: u64,
    checksum: u64,
}

graph!(GraphSmall, graph_small, Small, 48, build_small);
graph!(GraphBig, graph_big, Big, 1008, build_big);

fn main() {
    let w: usize = common::arg(1, "W");
    let steps: u64 = common::arg(2, "STEPS");
    let size: usize = common::arg(3, "SIZE");
    let degree: usize = common::arg(4, "DEGREE");
    let iters: u64 = common::arg(5, "ITERS");
    let seed: u64 = common::arg(6, "SEED");
    if w < 2 || degree == 0 || degree > w - 1 || (size != 64 && size != 1024) || steps == 0 {
        eprintln!("usage: scale_graph W>=2 STEPS>=1 SIZE in {{64,1024}} DEGREE in 1..W-1 ITERS SEED");
        std::process::exit(2);
    }
    let offs = graph_net::offsets(w as u64, degree as u64, seed);
    let mut t = Topology::new();
    let ids = if size == 64 {
        build_small(&mut t, w, &offs, steps, iters)
    } else {
        build_big(&mut t, w, &offs, steps, iters)
    };
    for id in ids {
        t.route::<Report>(id, Leader, 1);
    }
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    common::collect::<Report>(&run, w, |r| {
        println!(
            "{{\"bench\":\"graph\",\"workers\":{w},\"worker\":{},\"steps\":{steps},\"size\":{size},\"degree\":{degree},\"iters\":{iters},\"ns\":{},\"compute_ns\":{},\"checksum\":{}}}",
            r.worker, r.ns, r.compute_ns, r.checksum
        );
    });
    common::finish(run);
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

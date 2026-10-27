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

use trame::{Context, Message, Reading, Status, Topology, WorkerId, LEADER};

const WARM: u64 = 10;
const CAP: usize = 4;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report { worker: u64, ns: u64, compute_ns: u64, checksum: u64 }

graph!(GraphSmall, Small, 48, worker_small);
graph!(GraphBig, Big, 1008, worker_big);

/// Routes of one message type: worker i to each of its out-neighbours.
fn wire<M: Message>(t: &mut Topology, ids: &[WorkerId], i: usize, offs: &[u64]) {
    let w = ids.len();
    for &o in offs {
        t.route::<M>(ids[i], ids[(i + o as usize) % w], CAP);
    }
}

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
    let mut ids = Vec::new();
    for i in 0..w {
        let offs = offs.clone();
        let id = if size == 64 {
            t.worker(move |cx: &Context| worker_small(cx, i as u64, &offs, steps, iters))
        } else {
            t.worker(move |cx: &Context| worker_big(cx, i as u64, &offs, steps, iters))
        };
        ids.push(id);
    }
    for i in 0..w {
        if size == 64 { wire::<Small>(&mut t, &ids, i, &offs) } else { wire::<Big>(&mut t, &ids, i, &offs) }
        t.route::<Report>(ids[i], LEADER, 1);
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

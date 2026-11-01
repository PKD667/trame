// The out thread of a node's relay (NODES.md, The relay). It sleeps on the relay epoch. Each wake it
// scans this node's routes (relay_routes.rs) and control words and queues what is new, one buffer per
// destination node, then sends each non-empty buffer as one MPI message. Only this thread sends (relay.rs).
//
// Order within a scan. Records to one node leave in the order they were queued, and MPI keeps one pair's
// messages in order. The spec promises no delivery time across a barrier (a frame is available from its
// arrival time, spec 5.2), so a barrier record may overtake a frame of the same scan: that is allowed.

extern crate std;

use std::sync::atomic::Ordering::{Acquire, Release};
use std::vec;
use std::vec::Vec;

use super::relay::{Ctx, send};
use super::wire::{BATCH, Record, put};

/// The out thread's state. Only this thread touches it.
pub(super) struct Out<'a> {
    pub(super) ctx: &'a Ctx,
    /// Per route, outbound: the next frame to send (F, the relay's own cursor).
    pub(super) next: Vec<u64>,
    /// Per route, inbound lossless: the last credit sent.
    pub(super) credited: Vec<u64>,
    /// Per worker: its result is sent, and its exit is sent.
    pub(super) result_sent: Vec<bool>,
    pub(super) exit_sent: Vec<bool>,
    /// The last barrier generation this node has reported to the root.
    pub(super) reported: u64,
    /// This node has sent the abort bit.
    pub(super) abort_sent: bool,
    /// The root has sent END to every node.
    pub(super) end_sent: bool,
    /// Per node: the records queued for it in this scan.
    pub(super) bufs: Vec<Vec<u8>>,
    /// The route ids with `from` on this node and `to` not: frames go out on these.
    pub(super) outbound: Vec<u32>,
    /// The lossless route ids with `to` on this node and `from` not: credits go back on these.
    pub(super) inbound: Vec<u32>,
    /// One frame's payload, sized to the largest route, allocated once. A route uses its first `size` bytes.
    pub(super) scratch: Vec<u8>,
    /// Per route: the frames go over a lane, not MPI_Send (small.rs `is_small`, RMA.md).
    pub(super) lane: Vec<bool>,
    /// One lane message (head and payload), sized to the largest lane route, allocated once.
    pub(super) lane_buf: Vec<u8>,
}

/// The thread's body. Returns when the run has ended (`Ctx::ended`) and the last scan is sent.
pub(super) fn run(ctx: &Ctx) {
    let t = &ctx.port;
    let workers = t.lay().workers as usize;
    let routes = t.lay().routes.len();
    let nodes = t.nodes();
    let here = |rank: u32| nodes.node_of[rank as usize] == nodes.me;
    let (mut outbound, mut inbound) = (Vec::new(), Vec::new());
    for (id, r) in t.lay().routes.iter().enumerate() {
        if here(r.from) && !here(r.to) {
            outbound.push(id as u32);
        } else if here(r.to) && !here(r.from) && !r.lossy {
            inbound.push(id as u32);
        }
    }
    let largest = t.lay().routes.iter().map(|r| r.size).max().unwrap_or(0);
    let rma = ctx.ring.is_some();
    let lane: Vec<bool> = t
        .lay()
        .routes
        .iter()
        .map(|r| super::small::is_small(rma, r.from, r.to, r.size, r.lossy, &nodes.node_of))
        .collect();
    let largest_lane = t.lay().routes.iter().zip(&lane).filter(|(_, l)| **l).map(|(r, _)| r.size).max().unwrap_or(0);
    let mut out = Out {
        ctx,
        next: vec![0; routes],
        credited: vec![0; routes],
        result_sent: vec![false; workers],
        exit_sent: vec![false; workers],
        reported: 0,
        abort_sent: false,
        end_sent: false,
        bufs: vec![Vec::new(); nodes.count as usize],
        outbound,
        inbound,
        scratch: vec![0; largest],
        lane,
        lane_buf: vec![0; super::small::LANE_HEAD + largest_lane],
    };
    loop {
        let seen = t.relay_epoch();
        out.scan();
        if ctx.ended.load(Acquire) {
            break;
        }
        t.relay_sleep(seen);
    }
    // The last scan has sent the last lane frame. `rma-in` may now stop once the lanes are drained.
    ctx.out_done.store(true, Release);
}

impl Out<'_> {
    pub(super) fn scan(&mut self) {
        self.control();
        self.routes();
        for node in 0..self.bufs.len() {
            self.flush(node);
        }
    }

    /// Queue `r` for node `node`. A buffer over BATCH is sent first, and `r` starts the next one. A record
    /// that is larger than BATCH by itself goes out alone, as its own message (wire.rs BATCH).
    pub(super) fn push(&mut self, node: usize, r: &Record) {
        let buf = &mut self.bufs[node];
        let before = buf.len();
        put(buf, r);
        if buf.len() > BATCH {
            let tail = buf.split_off(before);
            self.flush(node);
            if tail.len() > BATCH {
                let dest = self.ctx.port.nodes().relay_rank[node];
                send(self.ctx.comm, dest, &tail);
            } else {
                self.bufs[node] = tail;
            }
        }
    }

    /// Send node `node`'s queued records as one message to its relay host.
    fn flush(&mut self, node: usize) {
        let buf = &mut self.bufs[node];
        if !buf.is_empty() {
            let dest = self.ctx.port.nodes().relay_rank[node];
            send(self.ctx.comm, dest, buf);
            buf.clear();
        }
    }
}

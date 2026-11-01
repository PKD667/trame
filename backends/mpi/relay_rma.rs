// The RMA side of a node's relay (RMA.md). Small frames between nodes go over one lane per ordered node
// pair, a `Ring::raw` of mpi-rma, instead of MPI_Send. `out` sends them (`send_lane`, from relay_routes.rs's
// scan), and `rma-in` polls the lanes that end here and applies each frame as relay_in.rs does for a p2p
// frame. Only the relay touches a lane: `out` on the sending node, `rma-in` on the receiving one.
//
// `rma-in` makes no MPI call. A lane is local memory on the receiving node, and the sender's Put completes
// (flush) before the sender moves on, so the frames of one lane arrive in order. Credits, barriers, results,
// aborts and END stay on p2p: the `in` thread blocks in MPI_Recv as before.

extern crate std;

use std::hint;
use std::sync::atomic::Ordering::{Acquire, Relaxed};
use std::sync::atomic::AtomicU64;
use std::thread;

use mpi_rma::Ring;

use super::layout::RouteLayout;
use super::relay::Ctx;
use super::relay_in::apply_frame;
use super::relay_out::Out;
use super::small::LANE_HEAD;

/// The lanes of this launch, on every rank of it (the window is collective). Shared by the relay's threads.
#[derive(Clone, Copy)]
pub(super) struct RingRef(pub(super) &'static Ring);

// SAFETY: `Ring::send` and `Ring::poll` take `&self` and keep their own state behind locks, and the lanes
// exist only in a launch over several nodes, which requires MPI_THREAD_MULTIPLE (open.rs), so `out` and
// `rma-in` may call them at once. The ring is leaked, never dropped: its window is never freed (NODES.md).
unsafe impl Send for RingRef {}
unsafe impl Sync for RingRef {}

/// Small frames this process's relay has sent by lane. The checks read it after finalization (check.rs).
static SENT: AtomicU64 = AtomicU64::new(0);

/// The number of small frames this process has sent by lane. Read after `finalize`, when the relay has stopped.
#[doc(hidden)]
pub fn lane_frames() -> u64 {
    SENT.load(Relaxed)
}

/// How many idle polls `rma-in` spins before it yields the processor to other threads.
const SPIN: u32 = 1000;

impl Out<'_> {
    /// Send frames F..P of a small outbound route, one lane message each, in index order. A lane is lossless
    /// and its frames are read from the route's own slots, so a frame that cannot be read is a bug.
    pub(super) fn send_lane(&mut self, id: usize, r: RouteLayout) {
        let ctx = self.ctx;
        let t = &ctx.port;
        let ring = ctx.ring.expect("trame: a small route exists only with its lane").0;
        let p = t.win().word(r.base).load(Acquire);
        let dest = t.nodes().relay_rank[t.nodes().node_of[r.to as usize] as usize];
        // The lane message is the head and the payload, built in a buffer that is reused for every frame.
        let mut buf = std::mem::take(&mut self.lane_buf);
        for index in self.next[id]..p {
            let read = t.read_slot(&r, index, Some(&mut buf[LANE_HEAD..LANE_HEAD + r.size]));
            if !read.unwrap_or_else(|e| panic!("trame: relay read of route {id}: {e}")) {
                panic!("trame: slot overwritten on a lossless route (route {id}, frame {index})");
            }
            buf[0..4].copy_from_slice(&(id as u32).to_le_bytes());
            buf[4..LANE_HEAD].copy_from_slice(&index.to_le_bytes());
            // `Ring::raw` never refuses a send: a lane cannot fill (RMA.md), so an error here is a bug.
            ring.send(dest, &buf[..LANE_HEAD + r.size])
                .unwrap_or_else(|e| panic!("trame: RMA lane send of route {id} frame {index} to rank {dest}: {e}"));
            SENT.fetch_add(1, Relaxed);
        }
        self.lane_buf = buf;
        self.next[id] = p;
    }
}

/// The thread's body. Polls the lanes that end here until `in` has seen END, then checks that no lane lost a
/// frame: a lost frame would be a bug in the lane plan (small.rs `lanes`), and is reported, not hidden.
pub(super) fn run_in(ctx: &Ctx) {
    let ring = ctx.ring.expect("trame: rma-in starts only with its lanes").0;
    let t = &ctx.port;
    let mut idle = 0u32;
    loop {
        // Read the flag before the poll: a frame sent before the flag is then seen by this poll or the next.
        let done = ctx.out_done.load(Acquire);
        let messages = ring.poll().unwrap_or_else(|e| panic!("trame: RMA lane poll: {e}"));
        if messages.is_empty() {
            if done {
                break;
            }
            idle = idle.saturating_add(1);
            if idle < SPIN {
                hint::spin_loop();
            } else {
                thread::yield_now();
            }
            continue;
        }
        idle = 0;
        for m in messages {
            let (head, payload) = m.data.split_at(LANE_HEAD);
            let route = u32::from_le_bytes(head[0..4].try_into().expect("four bytes"));
            let index = u64::from_le_bytes(head[4..LANE_HEAD].try_into().expect("eight bytes"));
            let r = t.lay().routes[route as usize];
            assert_eq!(payload.len(), r.size, "trame: lane frame {index} of route {route} has the wrong size");
            apply_frame(t, route, index, payload);
        }
    }
    let lost = ring.lost();
    assert_eq!(lost, 0, "trame: the RMA lanes lost {lost} frames");
}

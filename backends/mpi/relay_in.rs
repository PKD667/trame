// The in thread of a node's relay (NODES.md, The relay). It blocks in MPI_Recv and applies each record to
// this node's window, then wakes the local agents the record concerns. Only this thread receives. It
// keeps no state of its own: every effect is in the window.
//
// Each record is applied with the same stores a local agent would make, so a local waiter cannot tell
// a remote act from a local one. Frames use the seqlock writer (ring.rs `put`), and P is stored after it.

extern crate std;

use std::sync::atomic::Ordering::{self, AcqRel, Acquire, Relaxed};
use std::vec;
use std::vec::Vec;

use super::Transport;
use super::layout::{DONE, K_OUTCOME, LINE, RESULT};
use super::relay::{self, Ctx};
use super::wire::{Record, next};

/// The thread's body. Returns when END arrives.
pub(super) fn run(ctx: &Ctx) {
    // Grows to the largest message received (relay.rs `recv`).
    let mut buf: Vec<u8> = vec![];
    loop {
        let n = relay::recv(ctx.comm, &mut buf);
        let mut at = 0;
        while let Some((rec, used)) = next(&buf[at..n]) {
            at += used;
            if apply(ctx, rec) {
                return;
            }
        }
    }
}

/// Apply one record. Answers true for END, after which the thread stops.
fn apply(ctx: &Ctx, rec: Record) -> bool {
    let t: &Transport = &ctx.port;
    let workers = t.lay().workers;
    match rec {
        Record::Frames { route, index, bytes } => {
            let r = t.lay().routes[route as usize];
            t.put(&r, index, bytes).unwrap_or_else(|e| panic!("trame: relay frame {index} of route {route}: {e}"));
            let p = t.win().word(r.base);
            if p.load(Relaxed) < index + 1 {
                p.store(index + 1, Ordering::Release);
            }
            t.bump(r.to);
        }
        Record::Credit { route, c } => {
            let r = t.lay().routes[route as usize];
            let consumed = t.win().word(r.base + LINE);
            if consumed.load(Relaxed) < c {
                consumed.store(c, Ordering::Release);
            }
            t.bump(r.from);
        }
        Record::Result { worker, outcome } => {
            // A worker's result is stored once, as its own worker would (results.rs `finish`).
            let at = t.lay().per_worker(K_OUTCOME, worker);
            if t.win().word(at).compare_exchange(0, outcome, AcqRel, Acquire).is_ok() {
                t.win().word32(DONE).fetch_add(RESULT, AcqRel);
                t.win().wake(DONE);
            }
            if t.nodes().node_of[workers as usize] == t.nodes().me {
                t.bump(workers);
            }
        }
        Record::Abort { worker, bytes } => {
            // The flag first: `out` must see it before it sees the record, to not send the abort back.
            // A record already made here (a local panic first) stays; the bytes are then not used.
            ctx.aborted.store(true, Ordering::Release);
            t.record_abort(worker, bytes);
        }
        Record::End => {
            ctx.ended.store(true, Ordering::Release);
            t.bump_relay();
            return true;
        }
    }
    false
}

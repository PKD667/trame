// The in thread of a node's relay (NODES.md, The relay). It blocks in MPI_Recv and applies each record to
// this node's window, then wakes the local agents the record concerns. Only this thread receives. It
// keeps no state of its own beyond the root's count of arrivals: every other effect is in the window.
//
// Each record is applied with the same stores a local agent would make, so a local waiter cannot tell
// a remote act from a local one. Frames use the seqlock writer (ring.rs `put`), and P is stored after it.

extern crate std;

use std::sync::atomic::Ordering::{self, AcqRel, Acquire, Relaxed, SeqCst};
use std::vec;
use std::vec::Vec;

use super::Transport;
use super::layout::{COUNT, DONE, GEN, K_ARRIVE, K_EXITED, K_OUTCOME, LINE, PUB_NO, RESULT};
use super::publish::{deposit, set_publication};
use super::relay::{self, Ctx, Release};
use super::wire::{Record, next};

/// The thread's body. Returns when END arrives.
pub(super) fn run(ctx: &Ctx) {
    // Grows to the largest message received (relay.rs `recv`).
    let mut buf: Vec<u8> = vec![];
    // The root's count: the generation whose arrivals are being counted, and how many nodes have sent it.
    let mut round = (0u64, 0u32);
    loop {
        let n = relay::recv(ctx.comm, &mut buf);
        let mut at = 0;
        while let Some((rec, used)) = next(&buf[at..n]) {
            at += used;
            if apply(ctx, rec, &mut round) {
                return;
            }
        }
    }
}

/// Apply one record. Answers true for END, after which the thread stops.
fn apply(ctx: &Ctx, rec: Record, round: &mut (u64, u32)) -> bool {
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
        Record::Arrived { generation } => {
            // Root only: every node's workers have arrived at `generation`. Then the leader's deposit is
            // copied once, and `out` sends it with the release.
            if round.0 != generation {
                *round = (generation, 0);
            }
            round.1 += 1;
            if round.1 == t.nodes().arrivers {
                *round = (0, 0);
                let copied =
                    deposit(t.win(), None).unwrap_or_else(|e| panic!("trame: could not read the deposit: {e}"));
                let (no, bytes) = copied.expect("a deposit is copied when there is no `have`");
                *ctx.release.lock().expect("trame: the release lock") = Some(Release { generation, no, bytes });
                t.bump_relay();
            }
        }
        Record::Release { generation, no, bytes } => {
            // Install the publication, then add the other nodes' share of COUNT, which makes this node's
            // releaser the one that stores GEN (gate.rs). The local workers are then released.
            if no > t.win().word(PUB_NO).load(Relaxed) {
                set_publication(t.win(), no, bytes);
            }
            let share = u64::from(workers) - u64::from(t.nodes().local_workers);
            let c = t.win().word(COUNT).fetch_add(share, AcqRel) + share;
            assert_eq!(c % u64::from(workers), 0, "trame: release {generation} found COUNT {c} off a generation");
            assert_eq!(c / u64::from(workers), generation, "trame: release {generation} out of order");
            t.win().word(GEN).store(generation, SeqCst);
            bump_local(t, workers);
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
        Record::Exited { worker, arrive } => {
            // The arrive word first, so a waiter that sees EXITED sees it too (gate.rs `stuck`).
            t.win().word(t.lay().per_worker(K_ARRIVE, worker)).store(arrive, Ordering::Release);
            t.win().word(t.lay().per_worker(K_EXITED, worker)).store(1, Ordering::Release);
            bump_local(t, workers);
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

/// Bump every local agent among ranks `0..upto` (workers, then the leader's rank W). Walks the node's
/// own agents (`Nodes::local`, in rank order), not all ranks.
fn bump_local(t: &Transport, upto: u32) {
    for &w in t.nodes().local.iter().take_while(|&&w| w < upto) {
        t.bump(w);
    }
}

// Results, aborts and the start time, in the window. No polling: the leader's join sleeps on the DONE
// futex word, and every change of DONE wakes it.
//
// Each worker sets its own outcome word once, with a compare-and-swap from 0. The leader's join may
// also fill a zero word with Abort, after an abort, so the two never both win a word. A worker that
// adds its result adds RESULT to DONE and wakes the leader. A panicking worker sets ABORT_BIT first,
// so every waiter sees that the run is aborted at once. DONE is a u32 futex word. It never wraps: it
// grows by RESULT once per worker and by ABORT_BIT once, so it stays below 2 W + 2.
//
// Abort reaches a worker at its next effect (worker.rs), not while it computes: a worker that is
// running a loop with no effect in it keeps running until it makes one.

extern crate std;

use std::sync::atomic::Ordering::{AcqRel, Acquire, Release};
use std::vec::Vec;

use super::layout::{ABORT_BIT, DONE, END, K_OUTCOME, OUT_ABORT, RESULT, START};
use super::Transport;

impl Transport {
    /// Record this worker's result, unless one is already set: an abort may have set it first.
    /// Answers whether this call set it.
    pub fn finish(&self, outcome: u64) -> bool {
        let at = self.lay().per_worker(K_OUTCOME, self.me());
        if self.win().word(at).compare_exchange(0, outcome, AcqRel, Acquire).is_err() {
            return false;
        }
        self.win().word32(DONE).fetch_add(RESULT, AcqRel);
        self.win().wake(DONE);
        // A result is an act the leader waits for (LEADER_EPOCH, epoch.rs).
        self.bump(self.lay().workers);
        // Exit after the result: a waiter that sees the exit must also see the result (gate.rs).
        self.exit();
        true
    }

    /// Whether some agent panicked, which ends the run.
    pub fn aborted(&self) -> bool {
        self.win().word32(DONE).load(Acquire) & ABORT_BIT != 0
    }

    /// A worker panicked: the run is aborted, and this worker's own result is Abort.
    pub fn abort(&self) {
        self.win().word32(DONE).fetch_or(ABORT_BIT, AcqRel);
        self.win().wake(DONE);
        self.bump_all();
        self.finish(OUT_ABORT);
    }

    /// Leader only. Block until every worker has a result, or until the run aborts. After an abort,
    /// each worker without a result is given Abort here, and the leader does not wait for it.
    /// Answers one outcome word per worker, in rank order: each is nonzero.
    pub fn join(&self) -> Vec<u64> {
        let workers = self.lay().workers;
        loop {
            let seen = self.win().word32(DONE).load(Acquire);
            if seen & ABORT_BIT != 0 {
                for w in 0..workers {
                    let _ = self.win().word(self.lay().per_worker(K_OUTCOME, w)).compare_exchange(0, OUT_ABORT, AcqRel, Acquire);
                }
                break;
            }
            if seen / RESULT >= workers {
                break;
            }
            self.win().wait(DONE, seen);
        }
        (0..workers).map(|w| self.win().word(self.lay().per_worker(K_OUTCOME, w)).load(Acquire)).collect()
    }

    /// The launch's start time on `CLOCK_MONOTONIC`, written by the leader before the launch barrier.
    pub fn start(&self) -> u64 {
        self.win().word(START).load(Acquire)
    }

    /// Leader only, before the launch barrier.
    pub(super) fn set_start(&self, nanos: u64) {
        self.win().word(START).store(nanos, Release);
    }

    /// Leader only, after `join`: every result is in, so the relays may stop (NODES.md, End). A run on
    /// one node has no relay, and nothing is written.
    pub(super) fn end_run(&self) {
        if self.nodes().count > 1 {
            self.win().word(END).store(1, Release);
            self.bump_relay();
        }
    }
}

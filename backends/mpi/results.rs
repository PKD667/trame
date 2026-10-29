// Results, aborts and the start time, in the window. No polling: the leader's join sleeps on the DONE
// futex word, and every change of DONE wakes it.
//
// Each worker sets its own outcome word once, by a compare-and-swap from 0, and adds RESULT to DONE.
// DONE is a u32 futex word: it grows by RESULT per worker and sets ABORT_BIT once, so it never wraps.
//
// The abort record (`record_abort`). The first panic of a node is an object `<window>-abort-0`, created
// exclusively, so one panic is recorded per node. The worker's rank and the length are stored before
// ABORT_BIT, so a reader that sees the bit sees the whole message. A node that learns of an abort from the
// network records it the same way (relay_in.rs).
//
// Abort reaches a worker at its next effect (worker.rs), not while it computes.
//
// The leader. Every leader call checks the abort first (`fail_if_aborted`), and then panics with the
// worker and its message (spec.md 5, Abort). It ends the run first (`end_run`, `finalize`).

extern crate std;

use std::format;
use std::io;
use std::string::String;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Release};
use std::vec::Vec;

use super::Transport;
use super::layout::{ABORT_BIT, ABORT_LEN, ABORT_WORKER, DONE, END, K_OUTCOME, OUT_ABORT, RESULT, START};
use super::object;
use super::state::finalize;

impl Transport {
    /// Record this worker's result, unless one is already set. Answers whether this call set it.
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

    /// Record the run's abort on this node: the panicking worker's rank and its message, if this is the
    /// first record here. Answers whether this call recorded it. The message is stored, never truncated:
    /// if it cannot be stored, the record says so instead, and the run still ends. Wakes every agent.
    pub fn record_abort(&self, worker: u32, message: &[u8]) -> bool {
        let name = self.win().object("abort", 0);
        let stored = match object::create(&name, message) {
            Ok(()) => message.len(),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return false,
            Err(e) => {
                let text = format!("<the panic message could not be stored: {e}>");
                match object::create(&name, text.as_bytes()) {
                    Ok(()) => text.len(),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return false,
                    Err(e) => panic!("trame: could not record the abort: {e}"),
                }
            }
        };
        self.win().word(ABORT_WORKER).store(u64::from(worker), Release);
        self.win().word(ABORT_LEN).store(stored as u64, Release);
        self.win().word32(DONE).fetch_or(ABORT_BIT, AcqRel);
        self.win().wake(DONE);
        self.bump_all();
        true
    }

    /// A worker panicked with `message`: the run aborts, and this worker's own result is set, so the
    /// leader's join sees it as finished. The leader then panics (`fail_if_aborted`).
    pub fn abort(&self, message: &[u8]) {
        self.record_abort(self.me(), message);
        self.finish(OUT_ABORT);
    }

    /// The worker and the message of this node's abort record, for the relay to send. The bit must be
    /// set: the record is complete before it.
    pub(super) fn abort_record(&self) -> (u32, Vec<u8>) {
        let worker = self.win().word(ABORT_WORKER).load(Acquire) as u32;
        let len = self.win().word(ABORT_LEN).load(Acquire) as usize;
        let bytes = object::read(&self.win().object("abort", 0), len)
            .unwrap_or_else(|e| panic!("trame: could not read the abort message: {e}"));
        (worker, bytes)
    }

    /// Leader only. Block until every worker has a result, or until the run aborts. Answers one outcome
    /// word per worker, in rank order. After an abort the words may be unset: the caller checks
    /// `fail_if_aborted` before it reads them.
    pub fn join(&self) -> Vec<u64> {
        let workers = self.lay().workers;
        loop {
            let seen = self.win().word32(DONE).load(Acquire);
            // An OUT_ABORT outcome: the abort may reach this node after the results (relay_control.rs), so wait for the bit.
            let aborting =
                (0..workers).any(|w| self.win().word(self.lay().per_worker(K_OUTCOME, w)).load(Acquire) == OUT_ABORT);
            if seen & ABORT_BIT != 0 || (seen / RESULT >= workers && !aborting) {
                break;
            }
            self.win().wait(DONE, seen);
        }
        (0..workers).map(|w| self.win().word(self.lay().per_worker(K_OUTCOME, w)).load(Acquire)).collect()
    }

    /// Leader only. If the run aborted, end it the way `join` does, then panic with the text
    /// `trame: worker <w> panicked: <message>`. Otherwise do nothing. Every leader call starts with this.
    pub(super) fn fail_if_aborted(&self) {
        if !self.aborted() {
            return;
        }
        let w = self.win().word(ABORT_WORKER).load(Acquire);
        let len = self.win().word(ABORT_LEN).load(Acquire) as usize;
        // Read before the run ends: end_run unlinks the object.
        let message = match object::read(&self.win().object("abort", 0), len) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => format!("<the panic message could not be read: {e}>"),
        };
        self.end_run();
        finalize();
        panic!("trame: worker {w} panicked: {message}");
    }

    /// The launch's start time on `CLOCK_MONOTONIC`, written by the leader before the launch barrier.
    pub fn start(&self) -> u64 {
        self.win().word(START).load(Acquire)
    }

    /// Leader only, before the launch barrier.
    pub(super) fn set_start(&self, nanos: u64) {
        self.win().word(START).store(nanos, Release);
    }

    /// Leader only, after `join`: every result is in, so the relays may stop (NODES.md, End). On several
    /// nodes the relays unlink the publication objects when they stop (relay.rs `join`). On one node there
    /// is no relay, so this unlinks them: no worker is left in a barrier.
    pub(super) fn end_run(&self) {
        if self.nodes().count > 1 {
            self.win().word(END).store(1, Release);
            self.bump_relay();
        } else {
            self.unlink_publication();
        }
    }
}

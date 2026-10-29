// Wait conditions of agents. Each worker has an EPOCH word and the leader has LEADER_EPOCH. Every act
// that can change an agent's wait conditions moves its word (`bump`). An agent that must wait sleeps
// on its own word, and a bumper wakes it only when it is sleeping. Nobody polls.
//
// No lost wake-up. The waiter reads EPOCH (its `seen`), checks its conditions, stores SLEEP = 1, then
// reads EPOCH again; the bumper adds 1 to EPOCH, then loads SLEEP. All four accesses are SeqCst, so
// they have one total order S. If the bumper reads SLEEP = 0, its load comes before the waiter's
// store in S, so its add comes before the waiter's second read, and the waiter sees the new EPOCH and
// does not sleep. If the bumper reads SLEEP = 1, it wakes the futex. The kernel sleeps only while
// EPOCH still equals `seen`, so a bump between the check and the sleep makes the sleep return at once.
//
// Width. EPOCH is a u32, because the futex compares 32 bits. A bump between the waiter's SLEEP store
// and the kernel's compare wakes nobody, since nobody sleeps yet; the compare covers it, because EPOCH
// no longer equals `seen`. That fails only when EPOCH has wrapped back to `seen`: a multiple of 2^32
// bumps between the waiter's read and the compare, so at least 2^32. The bound is a count, not a rate,
// and nothing here measures how close a run gets. proofs/mpi/Epoch.tla checks both statements on a
// small word: no lost wake-up with 16 values and 3 bumps; a lost wake-up with 3 values at 3 bumps.

extern crate std;

use std::sync::atomic::Ordering::{Relaxed, SeqCst};
use std::time::Duration;

use super::Transport;
use super::layout::{K_EPOCH, K_SLEEP, LEADER_EPOCH, LEADER_SLEEP, RELAY_EPOCH, RELAY_SLEEP, START};

impl Transport {
    /// The epoch word and the sleep flag of agent `w`: a worker's, or the leader's when `w` is the
    /// leader's rank. An agent on another node is woken through this node's relay (NODES.md).
    fn words(&self, w: u32) -> (usize, usize) {
        if self.nodes().node_of[w as usize] != self.nodes().me {
            (RELAY_EPOCH, RELAY_SLEEP)
        } else if w >= self.lay().workers {
            (LEADER_EPOCH, LEADER_SLEEP)
        } else {
            (self.lay().per_worker(K_EPOCH, w), self.lay().per_worker(K_SLEEP, w))
        }
    }

    /// This worker's epoch, read before its wait conditions are checked (see `sleep`).
    pub(crate) fn epoch(&self) -> u32 {
        self.win().word32(self.words(self.me()).0).load(SeqCst)
    }

    /// An act that can change agent `w`'s wait conditions. `w` may be the leader's rank.
    pub(crate) fn bump(&self, w: u32) {
        let (e, s) = self.words(w);
        self.bump_word(e, s);
    }

    fn bump_word(&self, e: usize, s: usize) {
        self.win().word32(e).fetch_add(1, SeqCst);
        if self.win().word(s).load(SeqCst) != 0 {
            self.win().wake(e);
        }
    }

    /// The relay's epoch, read before its scan (NODES.md, relay): an act after the read makes the next
    /// `relay_sleep` return at once.
    pub(super) fn relay_epoch(&self) -> u32 {
        self.win().word32(RELAY_EPOCH).load(SeqCst)
    }

    /// Sleep while the relay epoch equals `seen`. The relay's own wait, the same protocol as `sleep`.
    pub(super) fn relay_sleep(&self, seen: u32) {
        self.sleep_on(RELAY_EPOCH, RELAY_SLEEP, seen, None);
    }

    /// An act the relay must act on: wakes it when it sleeps (the same protocol as `bump`).
    pub(super) fn bump_relay(&self) {
        self.bump_word(RELAY_EPOCH, RELAY_SLEEP);
    }

    /// Every agent's epoch moves: the run aborted, and every waiter must see it.
    pub(crate) fn bump_all(&self) {
        for w in 0..=self.lay().workers {
            self.bump(w);
        }
    }

    /// Sleep while this worker's epoch equals `seen`, until `until` (nanoseconds since the run
    /// started) if given. Returns when the epoch differs or the deadline has passed; the caller
    /// re-checks its conditions either way.
    pub(crate) fn sleep(&self, seen: u32, until: Option<u64>) {
        let (e, s) = self.words(self.me());
        self.sleep_on(e, s, seen, until);
    }

    /// The leader's wait (spec.md 5): block until its epoch differs from `seen`, and answer the new
    /// value. The caller reads the epoch before it checks its frames, so an act after that read
    /// makes the next wait return at once. `seen` is always a value this function returned, or 0.
    pub(crate) fn leader_wait(&self, seen: u64) -> u64 {
        loop {
            let e = self.win().word32(LEADER_EPOCH).load(SeqCst);
            if u64::from(e) != seen {
                return u64::from(e);
            }
            self.sleep_on(LEADER_EPOCH, LEADER_SLEEP, e, None);
        }
    }

    fn sleep_on(&self, e: usize, s: usize, seen: u32, until: Option<u64>) {
        loop {
            if self.win().word32(e).load(SeqCst) != seen {
                return;
            }
            let left = until.map(|t| t.saturating_sub(self.elapsed()));
            if left == Some(0) {
                return;
            }
            self.win().word(s).store(1, SeqCst);
            // The Dekker check: an act between the test above and the store is seen here, without a
            // syscall. The model (proofs/mpi/Epoch.tla) needs it for no property, because it puts the
            // kernel's compare in the SeqCst order; the hardware argument for that read is not modelled.
            if self.win().word32(e).load(SeqCst) != seen {
                self.win().word(s).store(0, Relaxed);
                return;
            }
            self.win().wait_for(e, seen, left.map(Duration::from_nanos));
            self.win().word(s).store(0, SeqCst);
        }
    }

    /// Nanoseconds since the run started, on `CLOCK_MONOTONIC` (see `clock` in types.rs).
    pub(crate) fn elapsed(&self) -> u64 {
        super::types::clock().saturating_sub(self.win().word(START).load(SeqCst))
    }
}

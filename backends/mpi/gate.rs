// The barrier in the shared window. No MPI call. The publication and the leader's deposit are in
// publish.rs; their bytes are POSIX objects (object.rs).
//
// Barrier. Generation g is each worker's g-th call. A worker stores g in its ARRIVE word, then adds 1
// to COUNT. The add that makes COUNT a multiple of N (workers) completes generation COUNT / N, and its
// worker is the releaser. The releaser installs the pending deposit (publish.rs), stores GEN, then bumps
// every worker's epoch. A waiter for g returns once GEN >= g.
// Several nodes. The releaser is the root's relay (NODES.md, Barrier): a worker whose add completes its
// node's arrivals bumps the relay instead, and the relay sends ARRIVED. A release arrives as RELEASE, and
// the relay adds the workers of the other nodes to COUNT, so the same COUNT rules hold on every node.
// Waiting. A waiter reads its epoch first, then checks GEN, the exits and the abort, and sleeps only on
// the epoch it read (epoch.rs). A release, an exit or an abort bumps every epoch after its write, so no
// waiter sleeps through it.
// Deadlock. A worker that returns sets its EXITED word, then bumps every epoch (`exit`). A waiter for g
// reports Deadlock when an exited worker's ARRIVE is below g: that worker never arrives at g, so g never
// completes, whatever anyone does (spec.md 5, Deadlock).

extern crate std;

use std::sync::Arc;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release, SeqCst};

use super::layout::{COUNT, GEN, K_ARRIVE, K_EXITED, PUB_LEN, PUB_NO};
use super::nodes::arrival_done;
use super::object;
use super::{Error, Transport};

impl Transport {
    /// Wait for every worker, then return the publication the releaser installed at this barrier, as a
    /// snapshot this worker may keep. Answers Deadlock when a worker returned without arriving, and
    /// Aborted when the run aborted.
    pub fn barrier(&mut self) -> Result<Arc<[u8]>, Error> {
        self.generation += 1;
        let g = self.generation;
        let n = u64::from(self.lay().workers);
        let arrive = self.lay().per_worker(K_ARRIVE, self.me());
        self.win().word(arrive).store(g, Release);
        let c = self.win().word(COUNT).fetch_add(1, AcqRel) + 1;
        if self.nodes().count == 1 && c.is_multiple_of(n) {
            self.install()?;
            self.win().word(GEN).store(c / n, SeqCst);
            for w in 0..self.lay().workers {
                self.bump(w);
            }
        } else if self.nodes().count > 1 && arrival_done(c, n, u64::from(self.nodes().local_workers)).is_some() {
            // The node's last local arrival: the relay sends ARRIVED to the root (NODES.md, Barrier).
            self.bump_relay();
        }
        self.wait_released(g)?;
        self.snapshot()
    }

    /// Sleep until generation `g` is released. Each pass reads the epoch first (see the module comment).
    fn wait_released(&self, g: u64) -> Result<(), Error> {
        loop {
            let seen = self.epoch();
            if self.win().word(GEN).load(SeqCst) >= g {
                return Ok(());
            }
            if self.stuck(g) {
                return Err(Error::Deadlock);
            }
            if self.aborted() {
                return Err(Error::Aborted);
            }
            self.sleep(seen, None);
        }
    }

    /// True when some worker has returned without arriving at generation `g`.
    fn stuck(&self, g: u64) -> bool {
        (0..self.lay().workers).any(|w| {
            // Exited first, then arrived: see the module comment.
            let gone = self.win().word(self.lay().per_worker(K_EXITED, w)).load(Acquire) == 1;
            gone && self.win().word(self.lay().per_worker(K_ARRIVE, w)).load(Acquire) < g
        })
    }

    /// The publication this worker reads. It reads the object only when PUB_NO moved since its last
    /// barrier: one copy per publication per worker, and none when the publication is unchanged. Its
    /// object is unlinked only after every worker has arrived past this barrier (publish.rs), so an
    /// object gone here means the run aborted and the leader ended it (`unlink_publication`).
    fn snapshot(&mut self) -> Result<Arc<[u8]>, Error> {
        let no = self.win().word(PUB_NO).load(Acquire);
        if let Some((seen, bytes)) = &self.publication
            && *seen == no
        {
            return Ok(bytes.clone());
        }
        let len = self.win().word(PUB_LEN).load(Relaxed) as usize;
        let name = self.win().object("pub", no);
        let bytes: Arc<[u8]> = match object::read(&name, len) {
            Ok(v) => Arc::from(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && self.aborted() => return Err(Error::Aborted),
            Err(e) => panic!("trame: could not read the publication {name}: {e}"),
        };
        self.publication = Some((no, bytes.clone()));
        Ok(bytes)
    }

    /// This worker has returned: set its EXITED word, then bump every epoch, so each waiter re-checks
    /// `stuck`. Called when the worker's result is recorded, before the process ends.
    pub(super) fn exit(&self) {
        self.win().word(self.lay().per_worker(K_EXITED, self.me())).store(1, Release);
        self.bump_all();
    }
}

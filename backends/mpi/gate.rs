// The barrier, the publication and the leader's deposit, in the shared window. No MPI call.
//
// Barrier. Generation g is each worker's g-th call. A worker stores g in its ARRIVE word, then adds 1
// to COUNT. The add that makes COUNT a multiple of N (workers) completes generation COUNT / N, and its
// worker is the releaser. The releaser installs the pending deposit (below), stores GEN, then bumps
// every worker's epoch. A waiter for g returns once GEN >= g.
// Waiting. A waiter reads its epoch first, then checks GEN, the exits and the abort, and sleeps only on
// the epoch it read (epoch.rs). A release, an exit or an abort bumps every epoch after its write, so no
// waiter sleeps through it.
// Deadlock. A worker that returns sets its EXITED word, then bumps every epoch (`exit`). A waiter for g
// reports Deadlock when an exited worker's ARRIVE is below g: that worker never arrives at g, so g never
// completes, whatever anyone does (spec.md 5, Deadlock).
//
// Publication. PUB_NO and PUB_LEN, PUB_BYTES are read by workers after a release. Only the releaser
// writes them, and it does so before it releases. A worker reads them after its release, and it
// arrives at the next generation only after that read; the next releaser writes only after every worker
// has arrived there. So no read races a write.
// Deposit. The leader writes PEND_BYTES under a seqlock, PEND_SEQ: odd while it writes, even when the
// deposit is complete. The releaser is the reader. It retries while the seqlock moved under its copy, and
// it sleeps on PEND_SEQ only while the word is odd. Why a seqlock and not a lock: the leader never waits,
// so Publish is non-blocking whatever the releaser does; and the releaser's wait is one bounded memcpy of
// the leader's, not a wait for the workers. A retry only follows a change of PEND_SEQ, so it does not spin.
//
// Words. Workers copy PUB and PEND with 8-byte atomic word accesses, never a plain copy: a torn read of a
// concurrently written word is then a defined value that the seqlock check rejects.

extern crate std;

use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release, SeqCst};
use std::sync::Arc;
use std::sync::atomic::fence;

use super::layout::{COUNT, GEN, K_ARRIVE, K_EXITED, PEND_BYTES, PEND_LEN, PEND_NO, PEND_SEQ, PUB_BYTES, PUB_LEN, PUB_MAX, PUB_NO};
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
        if c % n == 0 {
            self.install();
            self.win().word(GEN).store(c / n, SeqCst);
            for w in 0..self.lay().workers {
                self.bump(w);
            }
        }
        self.wait_released(g)?;
        Ok(self.snapshot())
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

    /// The publication this worker reads. It copies the bytes only when PUB_NO moved since its last
    /// barrier: one allocation per publication per worker, and none when the publication is unchanged.
    fn snapshot(&mut self) -> Arc<[u8]> {
        let no = self.win().word(PUB_NO).load(Acquire);
        if let Some((seen, bytes)) = &self.publication {
            if *seen == no {
                return bytes.clone();
            }
        }
        let len = self.win().word(PUB_LEN).load(Relaxed) as usize;
        let mut fresh = Arc::<[u8]>::new_uninit_slice(len);
        let dst = Arc::get_mut(&mut fresh).expect("a new Arc is unique");
        for (k, chunk) in dst.chunks_mut(8).enumerate() {
            let word = self.win().word(PUB_BYTES + 8 * k).load(Relaxed).to_le_bytes();
            for (j, b) in chunk.iter_mut().enumerate() {
                b.write(word[j]);
            }
        }
        // SAFETY: every byte of `fresh` was written by the loop above.
        let bytes = unsafe { fresh.assume_init() };
        self.publication = Some((no, bytes.clone()));
        bytes
    }

    /// Releaser only: if the deposit is newer than PUB, copy it into PUB and publish its number. The
    /// leader may be writing the deposit now, so the copy is validated against PEND_SEQ and retried.
    fn install(&self) {
        let win = self.win();
        let have = win.word(PUB_NO).load(Relaxed);
        loop {
            let s = win.word32(PEND_SEQ).load(Acquire);
            if s & 1 == 1 {
                win.wait(PEND_SEQ, s);
                continue;
            }
            let no = win.word(PEND_NO).load(Relaxed);
            if no == have {
                return;
            }
            // A torn length is clamped, not retried: the seqlock check below rejects the copy.
            let len = (win.word(PEND_LEN).load(Relaxed) as usize).min(PUB_MAX);
            for k in (0..len).step_by(8) {
                let w = win.word(PEND_BYTES + k).load(Relaxed);
                win.word(PUB_BYTES + k).store(w, Relaxed);
            }
            fence(Acquire);
            if win.word32(PEND_SEQ).load(Relaxed) == s {
                win.word(PUB_LEN).store(len as u64, Relaxed);
                win.word(PUB_NO).store(no, Release);
                return;
            }
        }
    }

    /// This worker has returned: set its EXITED word, then bump every epoch, so each waiter re-checks
    /// `stuck`. Called when the worker's result is recorded, before the process ends.
    pub(super) fn exit(&self) {
        self.win().word(self.lay().per_worker(K_EXITED, self.me())).store(1, Release);
        self.bump_all();
    }

    /// Leader only. Deposit `bytes` as the publication the next barrier that fires installs. Never waits:
    /// a deposit replaces an older one that no barrier has installed yet (latest wins).
    pub fn publish(&mut self, bytes: &[u8]) {
        assert!(self.me() == self.lay().workers, "only the leader publishes");
        assert!(
            bytes.len() <= PUB_MAX,
            "trame: leader_publish of {} bytes; the MPI backend's publication limit is {PUB_MAX} bytes",
            bytes.len()
        );
        let win = self.win();
        // The leader is the only writer of PEND_SEQ, so its own relaxed read is current.
        let s = win.word32(PEND_SEQ).load(Relaxed);
        win.word32(PEND_SEQ).store(s.wrapping_add(1), Relaxed);
        fence(Release);
        for (k, chunk) in bytes.chunks(8).enumerate() {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            win.word(PEND_BYTES + 8 * k).store(u64::from_le_bytes(word), Relaxed);
        }
        let no = win.word(PEND_NO).load(Relaxed) + 1;
        win.word(PEND_NO).store(no, Relaxed);
        win.word(PEND_LEN).store(bytes.len() as u64, Relaxed);
        win.word32(PEND_SEQ).store(s.wrapping_add(2), Release);
        win.wake(PEND_SEQ);
    }

    /// Leader only, before the launch barrier. Install the initial publication (spec.md 5.5, Pub_0)
    /// directly: no barrier has fired yet to install a deposit, and no worker has started.
    pub(super) fn install_initial(&self, bytes: &[u8]) {
        assert!(bytes.len() <= PUB_MAX, "initial publication over PUB_MAX");
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.win().ptr(PUB_BYTES, bytes.len()), bytes.len());
        }
        self.win().word(PUB_LEN).store(bytes.len() as u64, Relaxed);
    }
}

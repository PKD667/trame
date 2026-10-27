// The publication and the leader's deposit, in the window. Moved out of gate.rs to keep it readable; the
// protocol is the one stated there (the barrier) and in its module comment.
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
// Multi-node. A node's relay copies the deposit once, when the root has all nodes' arrivals (relay_in.rs),
// and installs the bytes it was sent with `set_publication`. The same protocol then holds per node.
//
// Words. Workers copy PUB and PEND with 8-byte atomic word accesses, never a plain copy: a torn read of a
// concurrently written word is then a defined value that the seqlock check rejects.

extern crate std;

use std::sync::atomic::fence;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::vec;
use std::vec::Vec;

use super::layout::{PEND_BYTES, PEND_LEN, PEND_NO, PEND_SEQ, PUB_BYTES, PUB_LEN, PUB_MAX, PUB_NO};
use super::shm::Window;
use super::Transport;

impl Transport {
    /// Releaser only: if the deposit is newer than PUB, copy it into PUB and publish its number. The
    /// leader may be writing the deposit now; `deposit` validates the copy.
    pub(super) fn install(&self) {
        let win = self.win();
        let have = win.word(PUB_NO).load(Relaxed);
        if let Some((no, bytes)) = deposit(win, Some(have)) {
            set_publication(win, no, &bytes);
        }
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

/// The leader's deposit, copied under its seqlock: `(number, bytes)`. `None` when `have` is its number
/// already, so an unchanged deposit is not copied. Waits only while the seqlock is odd (the leader is
/// writing), and retries only when the seqlock moved under the copy.
pub(super) fn deposit(win: &Window, have: Option<u64>) -> Option<(u64, Vec<u8>)> {
    loop {
        let s = win.word32(PEND_SEQ).load(Acquire);
        if s & 1 == 1 {
            win.wait(PEND_SEQ, s);
            continue;
        }
        let no = win.word(PEND_NO).load(Relaxed);
        if have == Some(no) {
            return None;
        }
        // A torn length is clamped, not retried: the seqlock check below rejects the copy.
        let len = (win.word(PEND_LEN).load(Relaxed) as usize).min(PUB_MAX);
        let mut bytes = vec![0u8; len];
        for (k, chunk) in bytes.chunks_mut(8).enumerate() {
            let word = win.word(PEND_BYTES + 8 * k).load(Relaxed).to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
        fence(Acquire);
        if win.word32(PEND_SEQ).load(Relaxed) == s {
            return Some((no, bytes));
        }
    }
}

/// Write publication `no` with `bytes` into PUB, then publish its number. The caller is the releaser of
/// the generation this publication belongs to: it runs before the release, so no worker reads PUB meanwhile.
pub(super) fn set_publication(win: &Window, no: u64, bytes: &[u8]) {
    for (k, chunk) in bytes.chunks(8).enumerate() {
        let mut word = [0u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        win.word(PUB_BYTES + 8 * k).store(u64::from_le_bytes(word), Relaxed);
    }
    win.word(PUB_LEN).store(bytes.len() as u64, Relaxed);
    win.word(PUB_NO).store(no, Release);
}

// The publication and the leader's deposit, in the window. The bytes are not in the window: each is a
// POSIX shared-memory object of its own length (object.rs), named `<window>-pub-<no>` or `<window>-pend-<no>`.
// The window keeps the numbers, lengths and the seqlock. The protocol is the one stated in gate.rs.
//
// Publication. PUB_NO and PUB_LEN are read by workers after a release. Only the releaser writes them,
// and it does so before it releases. A worker opens the object PUB_NO names after its release, and it
// arrives at the next generation only after that read. The releaser unlinks the previous object only
// after every worker has arrived at its generation, so no worker is still reading it.
// Deposit. The leader creates `pend-<no>` complete, then bumps PEND_NO and PEND_LEN under a seqlock,
// PEND_SEQ: odd while it writes, even when the deposit is complete. The releaser reads the number and
// length, opens the object, and checks the seqlock. It retries only when the seqlock moved, and it sleeps
// on PEND_SEQ only while the word is odd. A retry follows a change of PEND_SEQ, so it does not spin.
// Why a seqlock and not a lock: the leader never waits, so Publish is non-blocking whatever the releaser
// does; and the releaser's wait is one copy, not a wait for the workers.
// Multi-node. A node's relay copies the deposit once, when the root has all nodes' arrivals (relay_in.rs),
// and installs the bytes it was sent with `set_publication`. The same protocol then holds per node, on
// that node's objects.
// Unlinking. The previous deposit is unlinked by the leader after it bumps PEND_NO. A releaser that finds
// its object gone has seen the seqlock move, so it retries. The current objects are unlinked when the
// run is over (`unlink_publication`).

extern crate std;

use std::eprintln;
use std::io;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::fence;
use std::vec::Vec;

use super::Transport;
use super::layout::{PEND_LEN, PEND_NO, PEND_SEQ, PUB_LEN, PUB_NO};
use super::object;
use super::shm::Window;
use super::types::Error;

impl Transport {
    /// Releaser only: if the deposit is newer than the publication, install it. An `Aborted` answer means
    /// the run ended while the deposit was being read, so the barrier stops.
    pub(super) fn install(&self) -> Result<(), Error> {
        let win = self.win();
        let have = win.word(PUB_NO).load(Relaxed);
        match deposit(win, Some(have)) {
            Ok(Some((no, bytes))) => set_publication(win, no, &bytes),
            Ok(None) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound && self.aborted() => return Err(Error::Aborted),
            Err(e) => panic!("trame: could not read the leader's deposit: {e}"),
        }
        Ok(())
    }

    /// Leader only. Deposit `bytes` as the publication the next barrier that fires installs. Never waits:
    /// a deposit replaces an older one that no barrier has installed yet (latest wins). Panics when the
    /// object cannot be made, which the host's shared memory limit can cause.
    pub fn publish(&mut self, bytes: &[u8]) {
        assert!(self.me() == self.lay().workers, "only the leader publishes");
        let win = self.win();
        let no = win.word(PEND_NO).load(Relaxed) + 1;
        // The object is complete before PEND_NO names it, so a releaser that reads the number finds it.
        let name = win.object("pend", no);
        object::create(&name, bytes).unwrap_or_else(|e| panic!("trame: leader_publish of {} bytes: {e}", bytes.len()));
        // The leader is the only writer of PEND_SEQ, so its own relaxed read is current.
        let s = win.word32(PEND_SEQ).load(Relaxed);
        win.word32(PEND_SEQ).store(s.wrapping_add(1), Relaxed);
        fence(Release);
        win.word(PEND_NO).store(no, Release);
        win.word(PEND_LEN).store(bytes.len() as u64, Relaxed);
        win.word32(PEND_SEQ).store(s.wrapping_add(2), Release);
        win.wake(PEND_SEQ);
        // The previous deposit: a releaser that still wants it sees PEND_SEQ move, and retries.
        if let Err(e) = object::unlink(&win.object("pend", no - 1)) {
            eprintln!("trame: could not unlink an old deposit: {e}");
        }
    }

    /// Leader only, before the launch barrier. Create the initial publication (spec.md 5.5, Pub_0) and the
    /// empty deposit that PEND_NO = 0 names. No barrier has fired yet, and no worker has started.
    pub(super) fn install_initial(&self, bytes: &[u8]) -> io::Result<()> {
        let win = self.win();
        object::create(&win.object("pend", 0), &[])?;
        object::create(&win.object("pub", 0), bytes)?;
        win.word(PUB_LEN).store(bytes.len() as u64, Relaxed);
        Ok(())
    }

    /// Unlink the current publication and deposit objects of this window, and the abort message. Called only
    /// when no worker can still read them: after the leader's join (`end_run`, one node), or after the relays
    /// have stopped (`Relay::join`). An object that is already gone is not an error.
    pub(crate) fn unlink_publication(&self) {
        let win = self.win();
        let pub_no = win.word(PUB_NO).load(Acquire);
        let pend_no = win.word(PEND_NO).load(Acquire);
        for name in [win.object("pub", pub_no), win.object("pend", pend_no), win.object("abort", 0)] {
            if let Err(e) = object::unlink(&name) {
                eprintln!("trame: could not unlink {name}: {e}");
            }
        }
    }
}

/// The leader's deposit: `(number, bytes)`. `None` when `have` is its number already, so an unchanged
/// deposit is not copied. Waits only while the seqlock is odd (the leader is writing), and retries only
/// when the seqlock moved under the copy. A failed read with the seqlock unmoved is returned as an error.
pub(super) fn deposit(win: &Window, have: Option<u64>) -> io::Result<Option<(u64, Vec<u8>)>> {
    loop {
        let s = win.word32(PEND_SEQ).load(Acquire);
        if s & 1 == 1 {
            win.wait(PEND_SEQ, s);
            continue;
        }
        let no = win.word(PEND_NO).load(Acquire);
        if have == Some(no) {
            return Ok(None);
        }
        let len = win.word(PEND_LEN).load(Relaxed) as usize;
        let read = object::read(&win.object("pend", no), len);
        fence(Acquire);
        // A moved seqlock means the copy may be of a replaced deposit (or none): read again.
        if win.word32(PEND_SEQ).load(Relaxed) != s {
            continue;
        }
        return read.map(|bytes| Some((no, bytes)));
    }
}

/// Install publication `no` with `bytes`: create its object, then publish its number. The caller is the
/// releaser of the generation this publication belongs to; it runs before the release, so no worker reads
/// the new object before it is complete. The previous object is unlinked here: every worker read it before
/// it arrived at this generation, and this runs only after all have arrived.
pub(super) fn set_publication(win: &Window, no: u64, bytes: &[u8]) {
    let prev = win.word(PUB_NO).load(Relaxed);
    object::create(&win.object("pub", no), bytes)
        .unwrap_or_else(|e| panic!("trame: publication {no} of {} bytes: {e}", bytes.len()));
    win.word(PUB_LEN).store(bytes.len() as u64, Relaxed);
    win.word(PUB_NO).store(no, Release);
    if prev != no
        && let Err(e) = object::unlink(&win.object("pub", prev))
    {
        eprintln!("trame: could not unlink an old publication: {e}");
    }
}

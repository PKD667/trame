// Send on the rings of the shared window, and the seqlock frames both sides use.
//
// Lossless route (spec Send, Full). P is written only by the sender and C only by the receiver. The
// sender reads C with an acquire load; that load is where a Full is linearized: the answer is Full
// exactly when the value it read gives P - C == cap. The sender writes slot P mod cap, then publishes
// P + 1 with a release store, so the receiver's acquire load of P sees the whole frame.
//
// Lossy route (spec Overwrite, Drop). The sender never reads C and never waits: it writes slot
// P mod cap over the oldest frame. The receiver (recv.rs) keeps its own cursor and reads the newest
// cap frames. A slot is a seqlock: the sequence word is 2i+1 while frame i is being written and 2i+2
// once it is complete. A receiver that finds the sequence changed under its copy knows frame i was
// overwritten, and it skips it. Skipping is a Drop, and it is allowed because a newer frame exists.

extern crate std;

use std::sync::atomic::{
    Ordering::{Acquire, Relaxed, Release, SeqCst},
    fence,
};

use super::Transport;
use super::layout::{LINE, RouteLayout, SLOT_HEAD};
use super::types::{Answer, Error, Event, RouteId, clock};

pub(super) fn checked_seq(index: u64) -> Result<u64, Error> {
    index.checked_mul(2).and_then(|x| x.checked_add(2)).ok_or(Error::Broken("frame index overflows the sequence word"))
}

impl Transport {
    /// Send `msg` on route `id`, which this worker must send on.
    pub fn send(&mut self, id: RouteId, msg: &[u8]) -> Result<Answer, Error> {
        let r = *self.route(id)?;
        if r.from != self.me() {
            return Err(Error::NotYours(id));
        }
        if msg.len() != r.size {
            return Err(Error::BadSize { route: id, expected: r.size, got: msg.len() });
        }
        let tracing = self.trace.is_some();
        let p_at = r.base;
        let c_at = r.base + LINE;
        let before = if tracing { clock() } else { 0 };
        if tracing {
            // A timestamp does not show that an earlier store is visible, nor that a later load
            // has run. Full fences make the traced stamps bracket the load in memory order.
            fence(SeqCst);
        }
        // Only this worker writes P, so a relaxed read gives its own last value.
        let p = self.win().word(p_at).load(Relaxed);
        let c = if r.lossy { 0 } else { self.win().word(c_at).load(Acquire) };
        if tracing {
            fence(SeqCst);
        }
        let after = if tracing { clock() } else { 0 };
        let answer =
            if r.lossy || p.checked_sub(c).is_some_and(|used| used < r.cap) { Answer::Ok } else { Answer::Full };
        if answer == Answer::Ok {
            self.put(&r, p, msg)?;
            // Release: the frame's words are visible before P moves.
            self.win().word(p_at).store(p + 1, Release);
            self.bump(r.to);
        }
        self.push(Event::Send { route: id.0, index: p, read: c, before, after, answer });
        Ok(answer)
    }

    /// Write frame `index` into its slot as a seqlock writer: odd sequence, payload, even sequence.
    pub(super) fn put(&self, r: &RouteLayout, index: u64, msg: &[u8]) -> Result<(), Error> {
        let at = self.lay().slot(r, index);
        let done = checked_seq(index)?;
        let win = self.win();
        win.word(at).store(done - 1, Relaxed);
        fence(Release);
        for (k, chunk) in msg.chunks(8).enumerate() {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            win.word(at + SLOT_HEAD + 8 * k).store(u64::from_le_bytes(word), Relaxed);
        }
        win.word(at).store(done, Release);
        Ok(())
    }

    /// Read frame `index` as a seqlock reader, copying the payload into `out` when given. Returns
    /// false when the slot no longer holds frame `index`.
    pub(super) fn read_slot(&self, r: &RouteLayout, index: u64, out: Option<&mut [u8]>) -> Result<bool, Error> {
        let at = self.lay().slot(r, index);
        let want = checked_seq(index)?;
        let win = self.win();
        if win.word(at).load(Acquire) != want {
            return Ok(false);
        }
        if let Some(out) = out {
            for (k, chunk) in out.chunks_mut(8).enumerate() {
                let word = win.word(at + SLOT_HEAD + 8 * k).load(Relaxed).to_le_bytes();
                chunk.copy_from_slice(&word[..chunk.len()]);
            }
        }
        fence(Acquire);
        Ok(win.word(at).load(Relaxed) == want)
    }
}

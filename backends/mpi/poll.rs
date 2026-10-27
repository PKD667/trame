// Questions that take nothing: whether a frame is waiting for this endpoint, and whether a send
// would find room. Neither moves a route's head or tail, so both can be asked at any time. Both
// answer without yielding the baton in the sequential backend, and here they touch only the window.
//
// `room` is advisory for the sender's next send only: the receiver can free room, never take it,
// so a `true` stays true until this endpoint sends.

extern crate std;

use std::sync::atomic::Ordering::{Acquire, Relaxed};

use super::layout::LINE;
use super::{Error, RouteId, Transport};

impl Transport {
    /// Whether a frame is an offer to this endpoint on one of `owned`, which it must own.
    pub fn offer(&mut self, owned: &[RouteId]) -> Result<bool, Error> {
        for &id in owned {
            if self.route(id)?.to != self.me() {
                return Err(Error::NotYours(id));
            }
            if self.peek(id)?.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether a send on route `id`, which this endpoint must send on, would find room: a lossy
    /// route always does, and a lossless one does when fewer than `cap` frames are outstanding.
    pub fn room(&self, id: RouteId) -> Result<bool, Error> {
        let r = *self.route(id)?;
        if r.from != self.me() {
            return Err(Error::NotYours(id));
        }
        if r.lossy {
            return Ok(true);
        }
        // P is written only by this endpoint, so its own relaxed read is current. C is the
        // receiver's, read with acquire as in `send`.
        let p = self.win().word(r.base).load(Relaxed);
        let c = self.win().word(r.base + LINE).load(Acquire);
        Ok(p.checked_sub(c).is_some_and(|used| used < r.cap))
    }
}

// Receive on the rings of the shared window: the head of an owned route, and the lossy cursor.
//
// Receive (spec Offers). Frames are ordered within a route only. recv takes the head of the first
// owned route, scanning from just past the last route taken from that owned set. A route with a
// frame is taken within n receives: each receive that takes another route moves the scan start
// closer to it. A lossless head is consumed by publishing C; a lossy one is read by a cursor that
// only this endpoint keeps, and a head the sender has overwritten is skipped (a Drop).

extern crate std;

use std::sync::atomic::{fence, Ordering::{Acquire, Relaxed, Release, SeqCst}};

use super::layout::{RouteLayout, LINE};
use super::types::{clock, Error, Event, Received, RouteId};
use super::Transport;

impl Transport {
    /// Receive a frame from the owned routes, which this worker must own, into `out`. Takes the
    /// head of the first route in rotating order that has one. Returns `Empty` when none does.
    pub fn recv(&mut self, owned: &[RouteId], out: &mut [u8]) -> Result<Received, Error> {
        for &id in owned {
            if self.route(id)?.to != self.me() {
                return Err(Error::NotYours(id));
            }
        }
        let n = owned.len();
        if n == 0 {
            return Ok(Received::Empty);
        }
        // Keyed by the set's first route. Each route has one receiver and one tag, so the sets of two
        // tags never share a key, and each set keeps its own rotation.
        let key = owned[0].0 as usize;
        for k in 0..n {
            let slot = (self.rr[key] + k) % n;
            let id = owned[slot];
            let r = *self.route(id)?;
            if out.len() < r.size {
                return Err(Error::BadSize { route: id, expected: r.size, got: out.len() });
            }
            // A lossy head can be overwritten between peek and take. Then the next head is tried.
            while let Some(index) = self.peek(id)? {
                if self.take(id, &r, index, &mut out[..r.size])? {
                    self.rr[key] = (slot + 1) % n;
                    return Ok(Received::Message(id));
                }
            }
        }
        Ok(Received::Empty)
    }

    /// The index of the head of route `id`, or None when it has no frame.
    pub(super) fn peek(&mut self, id: RouteId) -> Result<Option<u64>, Error> {
        let r = *self.route(id)?;
        let p = self.win().word(r.base).load(Acquire);
        if r.lossy {
            let mut start = self.cursor_mut()[id.0 as usize].max(p.saturating_sub(r.cap));
            while start < p {
                if self.read_slot(&r, start, None)? {
                    return Ok(Some(start));
                }
                // Frame `start` was overwritten after P moved past it: drop it.
                let next = start + 1;
                self.cursor_mut()[id.0 as usize] = next;
                start = next.max(p.saturating_sub(r.cap));
            }
            Ok(None)
        } else {
            // Only this worker writes C, so a relaxed read gives its own last value.
            let c = self.win().word(r.base + LINE).load(Relaxed);
            if c < p {
                if !self.read_slot(&r, c, None)? {
                    return Err(Error::Broken("published frame has no valid sequence"));
                }
                Ok(Some(c))
            } else {
                Ok(None)
            }
        }
    }

    /// Copy frame `index` of `r` into `out` if it is still there. Returns `false` for a lossy
    /// frame that was overwritten, and then the cursor moves past it. Otherwise advances the
    /// consumer side of the route.
    fn take(&mut self, id: RouteId, r: &RouteLayout, index: u64, out: &mut [u8]) -> Result<bool, Error> {
        let tracing = self.trace.is_some();
        let before = if tracing { clock() } else { 0 };
        if !self.read_slot(r, index, Some(out))? {
            if !r.lossy {
                return Err(Error::Broken("published frame was overwritten"));
            }
            self.cursor_mut()[id.0 as usize] = index + 1;
            return Ok(false);
        }
        if r.lossy {
            self.cursor_mut()[id.0 as usize] = index + 1;
        } else {
            // Release: the sender may reuse this slot once C says it is free.
            self.win().word(r.base + LINE).store(index + 1, Release);
            self.bump(r.from);
            if tracing {
                fence(SeqCst);
            }
        }
        let after = if tracing { clock() } else { 0 };
        self.push(Event::Consume { route: id.0, index, before, after });
        Ok(true)
    }

}

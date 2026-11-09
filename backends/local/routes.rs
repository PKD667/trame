//! The routes of a backend whose agents share one address space (seq): each route a ring of
//! frames allocated at launch. Used under the backend's lock. INTERFACE.md 5.2: Send, Full, Overwrite, and
//! the fairness of takes across routes (CONTRACT.md, R2).

extern crate std;

use crate::io::Full;
use crate::launch::Route as Planned;
use crate::launch::Refused;
use crate::topology::End;
use std::any::TypeId;
use std::vec;
use std::vec::Vec;

/// A route with its frames. `from` sends, `to` receives; `ty` is its tag. The frames live in
/// `buf`, a ring of `cap` slots of `size` bytes, allocated once at launch: a send copies into a
/// slot and a receive copies out of one, so neither allocates. The oldest frame is at `head`.
pub(crate) struct Route {
    pub from: End,
    pub to: End,
    pub ty: TypeId,
    pub cap: usize,
    pub size: usize,
    pub lossy: bool,
    pub buf: Vec<u8>,
    pub head: usize,
    pub len: usize,
    /// The number of the take that last took a frame from this route; 0 before the first.
    pub taken: u64,
}

impl Route {
    fn slot(&self, at: usize) -> core::ops::Range<usize> {
        at * self.size..(at + 1) * self.size
    }
}

/// Every route of a run, and the count of takes.
pub(crate) struct Routes {
    pub all: Vec<Route>,
    /// The takes so far, numbering them for `Route::taken`.
    pub takes: u64,
}

impl Routes {
    /// Allocate every route's ring. Refused when one does not fit in memory.
    pub fn new(plan: &[Planned]) -> Result<Routes, Refused> {
        let mut all = Vec::with_capacity(plan.len());
        for r in plan {
            let bytes = r.cap.checked_mul(r.size).ok_or(Refused::Backend { reason: "a route's frames do not fit in memory".into() })?;
            all.push(Route { from: r.from, to: r.to, ty: r.ty, cap: r.cap, size: r.size, lossy: r.lossy, buf: vec![0; bytes], head: 0, len: 0, taken: 0 });
        }
        Ok(Routes { all, takes: 0 })
    }

    /// The index of the route from `from` to `to` with tag `ty`, if the topology has one.
    pub fn find(&self, from: End, to: End, ty: TypeId) -> Option<usize> {
        self.all.iter().position(|r| r.from == from && r.to == to && r.ty == ty)
    }

    /// Append a frame to route `i` (INTERFACE.md Send, Full, Overwrite). A lossy route at capacity
    /// overwrites its oldest frame; any other route at capacity answers `Full`.
    pub fn push(&mut self, i: usize, bytes: &[u8]) -> Result<(), Full> {
        let r = &mut self.all[i];
        let at = if r.len < r.cap {
            r.len += 1;
            (r.head + r.len - 1) % r.cap
        } else if r.lossy {
            let at = r.head;
            r.head = (r.head + 1) % r.cap;
            at
        } else {
            return Err(Full);
        };
        let range = r.slot(at);
        r.buf[range].copy_from_slice(bytes);
        Ok(())
    }

    /// Copy the oldest frame of a route to `to` with tag `ty` into `out`, and drop it from the route.
    /// Of the routes that have a frame, the one taken least recently is taken: a route that keeps a
    /// frame is then taken within one round of its siblings, whatever is taken in between (CONTRACT.md,
    /// R2). A cursor shared by every route and tag is not enough: a periodic mix of takes can
    /// bring it back to the same route each time. Answers `false` when no route has a frame.
    pub fn take(&mut self, to: End, ty: TypeId, out: &mut [u8]) -> bool {
        let next = self.takes + 1;
        let Some(r) = self.all.iter_mut().filter(|r| r.to == to && r.ty == ty && r.len > 0).min_by_key(|r| r.taken)
        else {
            return false;
        };
        let range = r.slot(r.head);
        out.copy_from_slice(&r.buf[range]);
        r.head = (r.head + 1) % r.cap;
        r.len -= 1;
        r.taken = next;
        self.takes = next;
        true
    }
}

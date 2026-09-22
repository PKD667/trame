//! The worker's end of a leader link, in host code.
//!
//! This is what the host model uses: a rank is a thread over ordinary memory, so the link is the
//! ring protocol driven by plain atomics, exactly as the leader's own end is.

use super::{Route, consume, publish};
use crate::contract::{Rank, Tag};
use crate::nv::error::{RecvError, SendError};
use crate::nv::layout::Layout;

pub struct Worker {
    up: *mut u32,
    down: *mut u32,
    layout: Layout,
    rank: Rank,
    departing: u32,
    arriving: u32,
}

impl Worker {
    /// # Safety
    ///
    /// `ptr` must address `route.words()` words that have been through `route.init`, live and
    /// unaliased for as long as either end is, and no other end may produce on this worker's up
    /// link or consume its down link.
    pub unsafe fn new(ptr: *mut u32, route: Route, rank: Rank) -> Self {
        Self {
            up: unsafe { route.up(ptr, rank) },
            down: unsafe { route.down(ptr, rank) },
            layout: route.layout(),
            rank,
            departing: 0,
            arriving: 0,
        }
    }

    /// # Safety
    ///
    /// As [`new`](Self::new), and no other thread may call this.
    pub unsafe fn send(&mut self, tag: Tag, data: &[u8]) -> Result<(), SendError> {
        unsafe { publish(self.up, self.layout, self.departing, self.rank, tag, data)? };
        self.departing = self.departing.wrapping_add(1);
        Ok(())
    }

    /// # Safety
    ///
    /// As [`send`](Self::send).
    pub unsafe fn recv(&mut self, out: &mut [u8]) -> Result<Option<(Tag, u32)>, RecvError> {
        let Some((_, tag, len)) = (unsafe { consume(self.down, self.layout, self.arriving, out)? })
        else {
            return Ok(None);
        };
        self.arriving = self.arriving.wrapping_add(1);
        Ok(Some((tag, len)))
    }
}

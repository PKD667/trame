//! The worker's end of a leader link, on a device.
//!
//! Same ring, same bytes, same protocol as the host end — what differs is who moves them. A worker
//! here is a warp, so the link is driven by the same warp-cooperative movers every other device
//! link uses, and the whole warp calls in convergence exactly as it does for a peer link.

use super::Route;
use crate::nv::device::{Rx, Tx};
use crate::nv::error::{RecvError, SendError};
use crate::nv::transport::Message;

pub struct Worker {
    tx: Tx,
    rx: Rx,
}

impl Worker {
    /// # Safety
    ///
    /// As the host end's, for this worker's two links: `ptr` must address `route.words()` words that
    /// have been through `route.init`, live and unaliased for as long as either end is, and no other
    /// end may produce on this worker's up link or consume its down link. All lanes of the warp must
    /// call the methods below in convergence with identical arguments.
    pub unsafe fn new(ptr: *mut u32, route: Route, rank: u32) -> Self {
        Self {
            tx: unsafe { Tx::new(route.up(ptr, rank), route.layout(), rank) },
            rx: unsafe { Rx::new(route.down(ptr, rank), route.layout()) },
        }
    }

    /// # Safety
    ///
    /// As the transport's own send, on this worker's up link.
    #[inline(always)]
    pub unsafe fn send(&mut self, tag: u32, data: &[u8]) -> Result<(), SendError> {
        unsafe { self.tx.send(tag, data.as_ptr(), data.len() as u32) }
    }

    /// # Safety
    ///
    /// As the transport's own receive, on this worker's down link.
    #[inline(always)]
    pub unsafe fn recv(&mut self, out: &mut [u8]) -> Result<Message, RecvError> {
        unsafe { self.rx.recv(out.as_mut_ptr(), out.len() as u32) }
    }
}

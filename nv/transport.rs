//! Transport abstraction: what a kernel needs from the NVMPI runtime.

use crate::contract::{Rank, Tag};
use crate::nv::error::{RecvError, SendError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub src: Rank,
    pub tag: Tag,
    pub len: u32,
}

pub mod sim;

#[cfg(feature = "cuda")]
pub mod cuda;

/// Largest rank count a transport table can hold. Kept fixed because the
/// device path cannot allocate.
pub const MAX_RANKS: usize = 32;

/// Spin budget for the blocking `send`/`recv` defaults. A saturated budget
/// surfaces the transient error instead of spinning forever.
pub const SPIN_BUDGET: u64 = 100_000_000;

/// Point-to-point messaging between ranks.
pub trait Transport {
    /// This rank's id, in `[0, size)`.
    fn rank(&self) -> Rank;
    /// Number of ranks.
    fn size(&self) -> Rank;

    /// One send attempt to `dst`.
    fn try_send(&mut self, dst: Rank, tag: Tag, data: &[u8]) -> Result<(), SendError>;
    /// One receive attempt from `src`.
    fn try_recv(&mut self, src: Rank, out: &mut [u8]) -> Result<Message, RecvError>;

    /// Sends, spinning on a full link.
    fn send(&mut self, dst: Rank, tag: Tag, data: &[u8]) -> Result<(), SendError> {
        let mut spins = 0;
        loop {
            match self.try_send(dst, tag, data) {
                Ok(()) => return Ok(()),
                Err(SendError::Full) => {
                    spins += 1;
                    if spins >= SPIN_BUDGET {
                        return Err(SendError::Full);
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Receives, spinning on an empty link.
    fn recv(&mut self, src: Rank, out: &mut [u8]) -> Result<Message, RecvError> {
        let mut spins = 0;
        loop {
            match self.try_recv(src, out) {
                Ok(m) => return Ok(m),
                Err(RecvError::Empty) => {
                    spins += 1;
                    if spins >= SPIN_BUDGET {
                        return Err(RecvError::Empty);
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}

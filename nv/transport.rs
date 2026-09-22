//! Transport abstraction: what a kernel needs from the NVMPI runtime.

use crate::nv::error::{RecvError, SendError};

/// One received frame's header. Ranks and tags are raw slot words below the contract boundary;
/// `nv/mod.rs` and `leader.rs` convert them to `Rank` and `Tag`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub src: u32,
    pub tag: u32,
    pub len: u32,
}

pub mod sim;

#[cfg(feature = "cuda")]
pub mod cuda;

/// Largest rank count a transport table can hold. Kept fixed because the
/// device path cannot allocate.
pub const MAX_RANKS: usize = 32;

/// Point-to-point messaging between ranks.
pub trait Transport {
    /// This rank's id, in `[0, size)`.
    fn rank(&self) -> u32;
    /// Number of ranks.
    fn size(&self) -> u32;

    /// One send attempt to `dst`.
    fn try_send(&mut self, dst: u32, tag: u32, data: &[u8]) -> Result<(), SendError>;
    /// One receive attempt from `src`.
    fn try_recv(&mut self, src: u32, out: &mut [u8]) -> Result<Message, RecvError>;
}

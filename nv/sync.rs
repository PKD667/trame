//! Shared-state families for warps of one launch.
//!
//! The names match `trame::cpu::sync`; storage does not. A host constructor may allocate its
//! fixed buffers, while a device launch must prepare every metadata word and payload slot before
//! any warp starts. The constructors in Family B are therefore unsafe views over caller-owned
//! storage, but their handles make the same ownership promises after construction.

pub mod atomic;
pub mod handoff;
pub mod publish;
pub mod turn;

pub use handoff::{Idle, Receiver, Refused, Sender, Unsent, handoff, stocked};
pub use publish::{Ended, Pinned, Pressure, Reader, Version, Writer, published};
pub use turn::{Cancel, Exclusive, NORMAL, Priority, Turn, WAITERS, WORDS, init_header};

/// A retry-loop hint where the target has one. PTX has no corresponding pause intrinsic; the
/// atomic operation surrounding this call is the progress operation on a device.
#[inline(always)]
pub(crate) fn spin() {
    #[cfg(not(feature = "cuda"))]
    core::hint::spin_loop();
}

// CPU implementations of the shared-state primitives a step uses. Every working-path call makes
// one attempt and reports contention instead of waiting, because a step may not block.

pub mod atomic;
pub mod handoff;
mod turn;

pub use bytemuck::NoUninit;
pub use turn::{Exclusive, Locked};

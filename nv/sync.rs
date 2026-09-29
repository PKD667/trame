//! Owned shared-state primitives in one worker's valid address domain.
//! Each acquisition or transfer belongs to its caller; no Rust value is lane-broadcast.

pub mod atomic;
pub mod handoff;
mod turn;

pub use turn::{Exclusive, Locked, with};

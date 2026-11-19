//! Bounded collections: a vector and a heap whose capacity is fixed at construction and never changes.
//!
//! `new` is the only function here that allocates. Every other method on `Bounded` and `BoundedHeap` is
//! safe to call from a step, which may neither allocate nor free (INTERFACE.md 3.1): a method that would
//! need more room answers `Err(Full)` instead of growing the buffer. The buffer is `Vec::with_capacity(cap)`
//! made once, and no method calls `reserve`, `shrink_to_fit` or anything that reallocates, so its address
//! is the same from `new` until the value is dropped. Dropping a `Bounded` frees the buffer: do that in the
//! builder or an edit, never in a step.
//!
//! `Full` is `trame::Full` (the answer of `send`, INTERFACE.md 5.2): one name for "at capacity" everywhere.

mod heap;
mod vec;

#[cfg(test)]
mod tests;

pub use heap::BoundedHeap;
pub use vec::Bounded;

use crate::io::Full;

impl core::fmt::Display for Full {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("full: at capacity")
    }
}

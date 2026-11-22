//! `Bounded<T>`: a vector of fixed capacity (see mod.rs for the rule that makes it safe in a step).

use super::Full;
use alloc::vec::Vec;
use core::fmt;
use core::ops::{Deref, DerefMut};

/// A vector with `cap` slots, allocated once in `new`. Invariant: `buf.len() <= cap` and `buf.capacity()`
/// is never changed, so no method reallocates. `T: Copy` on the methods that remove or overwrite values
/// (`pop`, `clear`, `truncate`, `swap_remove`, `push`, `extend_from_slice`): removing a `Copy` value drops
/// nothing, so no method frees.
pub struct Bounded<T> {
    pub(super) buf: Vec<T>,
    pub(super) cap: usize,
}

impl<T> Bounded<T> {
    /// An empty `Bounded` with room for `cap` values. Allocates; call it in a builder or an edit, never in
    /// a step. Panics as `Vec::with_capacity` does when `cap` values of `T` cannot be allocated.
    pub fn new(cap: usize) -> Self {
        Bounded { buf: Vec::with_capacity(cap), cap }
    }

    /// The most values this holds.
    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.buf.len() == self.cap
    }

    /// Appends `value`, or hands it back when the buffer is full. Use this for non-`Copy` `T`: the caller
    /// keeps the value and decides what to do with it.
    pub fn push_owned(&mut self, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        // len < cap <= buf.capacity(): this does not reallocate.
        self.buf.push(value);
        Ok(())
    }

    pub fn as_slice(&self) -> &[T] {
        &self.buf
    }

    /// The buffer, for `Plain::blocks` (src/plain.rs).
    pub(crate) fn vec(&self) -> &Vec<T> {
        &self.buf
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.buf
    }
}

impl<T: Copy> Bounded<T> {
    /// Appends `value`, or answers `Full` and leaves the contents unchanged.
    pub fn push(&mut self, value: T) -> Result<(), Full> {
        self.push_owned(value).map_err(|_| Full)
    }

    /// Removes and returns the last value.
    pub fn pop(&mut self) -> Option<T> {
        self.buf.pop()
    }

    /// Removes every value. Frees nothing: the capacity stays.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Keeps the first `n` values; does nothing when `n >= len`. Frees nothing.
    pub fn truncate(&mut self, n: usize) {
        self.buf.truncate(n);
    }

    /// Removes the value at `i` by moving the last value into its place. `None` when `i >= len`.
    pub fn swap_remove(&mut self, i: usize) -> Option<T> {
        if i < self.buf.len() { Some(self.buf.swap_remove(i)) } else { None }
    }

    /// Appends all of `values`, or none of them: answers `Full` and leaves the contents unchanged when they
    /// do not all fit.
    pub fn extend_from_slice(&mut self, values: &[T]) -> Result<(), Full> {
        if values.len() > self.cap - self.buf.len() {
            return Err(Full);
        }
        // values.len() <= cap - len: the reserve inside Vec::extend_from_slice finds room; no reallocation.
        self.buf.extend_from_slice(values);
        Ok(())
    }
}

impl<T> Deref for Bounded<T> {
    type Target = [T];

    /// Every slice method works through this: indexing, `get`, `get_mut`, `first`, `last`, `iter`,
    /// `iter_mut`, `contains`, `sort`, and the rest. None of them changes the length.
    fn deref(&self) -> &[T] {
        &self.buf
    }
}

impl<T> DerefMut for Bounded<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.buf
    }
}

impl<T: fmt::Debug> fmt::Debug for Bounded<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bounded").field("cap", &self.cap).field("items", &self.as_slice()).finish()
    }
}

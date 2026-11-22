//! `BoundedHeap<T>`: a binary max-heap over a `Bounded<T>`, with the same fixed capacity and no allocation.

use super::{Bounded, Full};

/// A max-heap of at most `cap` values, stored in a `Bounded`. Written by hand: `alloc::collections::BinaryHeap`
/// grows, so it cannot be used in a step. For a min-heap, use `BoundedHeap<core::cmp::Reverse<T>>`.
///
/// Ties. `sift_up` moves a value up only when it is strictly greater than its parent. `sift_down` picks the
/// right child only when it is strictly greater than the left, and moves a value down only when the chosen
/// child is strictly greater. So the layout is a function of the sequence of operations, the same on every
/// machine. Among values that compare equal, `pop` returns whichever the layout puts first; for keys that
/// are equal as whole values (every `u32`, say) that is indistinguishable, and for a type whose `Ord` looks
/// at only part of the value, it is not FIFO and not specified beyond this rule.
#[derive(Debug)]
pub struct BoundedHeap<T> {
    pub(super) items: Bounded<T>,
}

impl<T> BoundedHeap<T> {
    /// The buffer, for `Plain::blocks` (src/plain.rs).
    pub(crate) fn vec(&self) -> &alloc::vec::Vec<T> {
        self.items.vec()
    }
}

impl<T: Ord + Copy> BoundedHeap<T> {
    /// An empty heap with room for `cap` values. Allocates; see `Bounded::new`.
    pub fn new(cap: usize) -> Self {
        BoundedHeap { items: Bounded::new(cap) }
    }

    pub fn cap(&self) -> usize {
        self.items.cap()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.items.is_full()
    }

    /// The largest value, or `None` when empty.
    pub fn peek(&self) -> Option<&T> {
        self.items.first()
    }

    /// The values in heap order (the root first). The order is fixed by the operations, but not sorted.
    pub fn as_slice(&self) -> &[T] {
        self.items.as_slice()
    }

    /// Adds `value`, or answers `Full` and leaves the heap unchanged.
    pub fn push(&mut self, value: T) -> Result<(), Full> {
        self.items.push(value)?;
        let last = self.items.len() - 1;
        self.sift_up(last);
        Ok(())
    }

    /// Removes and returns the largest value.
    pub fn pop(&mut self) -> Option<T> {
        let top = self.items.swap_remove(0)?;
        self.sift_down(0);
        Some(top)
    }

    /// Pushes `value` and pops the largest of the heap and `value`: never fails, even when full, and the
    /// length does not change while the heap is non-empty. A value equal to the top is returned and the
    /// heap is left as it was. An empty heap returns `value` at once.
    pub fn push_pop(&mut self, value: T) -> T {
        match self.items.as_mut_slice().first_mut() {
            Some(top) if value < *top => {
                let old = core::mem::replace(top, value);
                self.sift_down(0);
                old
            }
            _ => value,
        }
    }

    /// Removes every value. Frees nothing.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    fn sift_up(&mut self, mut i: usize) {
        let s = self.items.as_mut_slice();
        while i > 0 {
            let parent = (i - 1) / 2;
            if s[i] > s[parent] {
                s.swap(i, parent);
                i = parent;
            } else {
                break;
            }
        }
    }

    fn sift_down(&mut self, mut i: usize) {
        let s = self.items.as_mut_slice();
        let n = s.len();
        // The children of i are 2i+1 and 2i+2. checked_* keeps this total for any n, including zero-sized T,
        // where n can be as large as usize::MAX.
        while let Some(mut child) = i.checked_mul(2).and_then(|x| x.checked_add(1)).filter(|&c| c < n) {
            let right = child + 1; // child < n <= usize::MAX, so this does not overflow.
            if right < n && s[right] > s[child] {
                child = right;
            }
            if s[child] > s[i] {
                s.swap(i, child);
                i = child;
            } else {
                break;
            }
        }
    }
}

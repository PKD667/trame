//! The runtime of `parallel!` (spec.md 5.4), shared by every backend. It runs the items in one
//! thread. That is a valid schedule: keys are independent, and the items of one key run in input
//! order.
//!
//! The grouping is a counting sort of item indices by key. Its buffers belong to the calling
//! thread and are kept between calls, so after the first call of a given size a call allocates
//! nothing.

extern crate std;

// The runtime of `concurrent!` is `concurrent.rs`; its items are re-exported here so every
// runtime entry is `trame::__rt::...`, as the driver's effects.txt names it.
pub use crate::cells::{Cells, MAX_OWNED, Owning, Pick, Tags, cell_ty, owns};
pub use crate::concurrent::{MAX_PROCESSES, Process, Set, concurrent, offered};

use core::cell::RefCell;
use core::mem::take;
use std::thread_local;
use std::vec::Vec;

/// The buffers of one call. `ends[k]` is the end of key `k`'s run in `order`, and, after the
/// placement, its start.
#[derive(Default)]
struct Buffers {
    keys: Vec<usize>,
    ends: Vec<usize>,
    order: Vec<usize>,
}

thread_local! {
    static BUFFERS: RefCell<Buffers> = const {
        RefCell::new(Buffers { keys: Vec::new(), ends: Vec::new(), order: Vec::new() })
    };
}

/// `parallel!` (spec.md 5.4): for each key `k`, fold the items whose key is `k` into `slots[k]`,
/// in input order. Aborts before any item runs, unless every key is below `slots.len()`, as `x[i]`
/// does (spec.md 5.4, Parallel effect).
pub fn parallel<I, S, C, K, F>(key: K, f: F, cx: &C, items: &[I], slots: &mut [S])
where
    K: Fn(&I) -> usize,
    F: Fn(&I, &mut S, &C),
{
    // Take the buffers out, so a call made inside `f` finds them empty and allocates its own,
    // rather than borrowing the same cell twice.
    let mut b = BUFFERS.with(|cell| take(&mut *cell.borrow_mut()));
    run(&mut b, &key, &f, cx, items, slots);
    BUFFERS.with(|cell| *cell.borrow_mut() = b);
}

fn run<I, S, C, K, F>(b: &mut Buffers, key: &K, f: &F, cx: &C, items: &[I], slots: &mut [S])
where
    K: Fn(&I) -> usize,
    F: Fn(&I, &mut S, &C),
{
    let (n, m) = (slots.len(), items.len());
    b.keys.clear();
    for item in items {
        let k = key(item);
        // Checked for every item before the first runs: a bad key aborts with no item run (spec.md 5.4).
        assert!(
            k < n,
            "trame: parallel!: an item has key {k}, but there are {n} slots; a key must be below the slot count, as x[i] is. No item ran."
        );
        b.keys.push(k);
    }
    // Counting sort, stable: count each key, make the counts cumulative, then place the indices
    // from the last to the first, so the earliest index of a key lands first in its run.
    b.ends.clear();
    b.ends.resize(n, 0);
    for &k in &b.keys {
        b.ends[k] += 1;
    }
    for k in 1..n {
        b.ends[k] += b.ends[k - 1];
    }
    b.order.clear();
    b.order.resize(m, 0);
    for i in (0..m).rev() {
        let k = b.keys[i];
        b.ends[k] -= 1;
        b.order[b.ends[k]] = i;
    }
    // Now `ends[k]` is the start of key k's run, and the start of key k+1 ends it.
    for (k, slot) in slots.iter_mut().enumerate().take(n) {
        let begin = b.ends[k];
        let end = if k + 1 < n { b.ends[k + 1] } else { m };
        for &i in &b.order[begin..end] {
            f(&items[i], slot, cx);
        }
    }
}

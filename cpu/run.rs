// The host lowering of `#[parallel]` and `#[concurrent]`: an in-order loop on the calling thread,
// and one scoped thread per item. Every item runs; the first `Err` in list order is the answer.

use crate::{Invoked, Keyed};

pub fn parallel<I: Copy, C, E>(
    cx: &mut C,
    items: &[I],
    mut body: impl FnMut(I, &mut C) -> Result<(), E>,
) -> Result<(), E> {
    let mut first = Ok(());
    for &item in items {
        let outcome = body(item, cx);
        if first.is_ok() {
            first = outcome;
        }
    }
    first
}

/// Issue order is list order, so every key keeps it.
pub fn ordered<I: Copy, K: Copy + Into<usize>, T, C, E>(
    cx: &mut C,
    items: &[I],
    mut keyed: Keyed<'_, K, T>,
    key: impl Fn(I) -> K,
    mut body: impl FnMut(I, &mut T, &mut C) -> Result<(), E>,
) -> Result<(), Invoked<E>> {
    let mut first = Ok(());
    for &item in items {
        let outcome = match keyed.slot(key(item)) {
            Ok(slot) => body(item, slot, cx).map_err(Invoked::Failed),
            Err(out) => Err(out),
        };
        if first.is_ok() {
            first = outcome;
        }
    }
    first
}

/// A panicking item is joined with the rest and its panic resumed, as `std::thread::scope` does.
pub fn concurrent<I: Copy + Send, C: Sync, E: Send>(
    cx: &C,
    items: &[I],
    body: impl Fn(I, &C) -> Result<(), E> + Sync,
) -> Result<(), E> {
    let body = &body;
    std::thread::scope(|scope| {
        let running: Vec<_> = items
            .iter()
            .map(|&item| scope.spawn(move || body(item, cx)))
            .collect();
        let mut first = Ok(());
        for thread in running {
            let outcome = thread
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            if first.is_ok() {
                first = outcome;
            }
        }
        first
    })
}

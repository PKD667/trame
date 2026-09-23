// The host lowering of `#[parallel]`, an in-order loop on the calling thread, and of
// `concurrent!`, one scoped thread per arm.

use std::panic::resume_unwind;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::thread::{self, Scope, ScopedJoinHandle};

use crate::{Invoked, Keyed, Step};

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

/// Fixes an arm's closure signature where it is written; `Send` keeps it portable across lowerings.
#[inline(always)]
pub fn arm<F, E>(arm: F) -> F
where
    F: FnMut() -> Result<Step, E> + Send,
{
    arm
}

pub struct Arms<'scope, 'env: 'scope, E, const N: usize> {
    scope: &'scope Scope<'scope, 'env>,
    /// Shared rather than borrowed: `'env` is the caller's, and this call's locals do not live that long.
    cut: Arc<AtomicBool>,
    running: [Option<ScopedJoinHandle<'scope, Result<(), E>>>; N],
    at: usize,
}

impl<'scope, 'env, E: Send + 'scope, const N: usize> Arms<'scope, 'env, E, N> {
    pub fn arm<F>(&mut self, arm: &'scope mut F)
    where
        F: FnMut() -> Result<Step, E> + Send,
    {
        let cut = Arc::clone(&self.cut);
        self.running[self.at] = Some(self.scope.spawn(move || drive(&cut, arm)));
        self.at += 1;
    }
}

/// A panicking arm is joined with the rest and its panic resumed; it takes precedence over errors.
pub fn concurrent<'env, B, E: Send, const N: usize>(body: B) -> Result<(), E>
where
    B: for<'scope> FnOnce(&mut Arms<'scope, 'env, E, N>),
{
    thread::scope(|scope| {
        let mut arms = Arms {
            scope,
            cut: Arc::new(AtomicBool::new(false)),
            running: std::array::from_fn(|_| None),
            at: 0,
        };
        // A panic in `body` after an arm spawned is joined by the scope, so it is published too:
        // an arm that is `Idle` forever would otherwise hang that join.
        let cut = Arc::clone(&arms.cut);
        let _cut = CutOnPanic(&cut);
        body(&mut arms);
        let mut panic = None;
        let mut first = Ok(());
        for thread in arms.running.into_iter().flatten() {
            match thread.join() {
                Err(payload) => {
                    panic.get_or_insert(payload);
                }
                Ok(outcome) => {
                    if first.is_ok() {
                        first = outcome;
                    }
                }
            }
        }
        if let Some(payload) = panic {
            resume_unwind(payload);
        }
        first
    })
}

/// The cutoff is the check before each step: a step already past it may run after a sibling
/// publishes an error. The flag carries no data, so it needs no ordering beyond its own word.
fn drive<F, E>(cut: &AtomicBool, arm: &mut F) -> Result<(), E>
where
    F: FnMut() -> Result<Step, E>,
{
    let _cut = CutOnPanic(cut);
    loop {
        if cut.load(Relaxed) {
            return Ok(());
        }
        match arm() {
            Ok(Step::Progress) => {}
            Ok(Step::Idle) => thread::yield_now(),
            Ok(Step::Done) => return Ok(()),
            Err(e) => {
                cut.store(true, Relaxed);
                return Err(e);
            }
        }
    }
}

/// A panic is published like an error, so siblings stop starting steps and the join can end.
struct CutOnPanic<'a>(&'a AtomicBool);

impl Drop for CutOnPanic<'_> {
    fn drop(&mut self) {
        if thread::panicking() {
            self.0.store(true, Relaxed);
        }
    }
}

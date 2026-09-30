// The host lowering of `#[parallel]`, inline on the caller's thread, and of `concurrent!`, one
// scoped thread per arm.

use std::panic::resume_unwind;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::thread::{self, Scope, ScopedJoinHandle};

use crate::{Invoked, Keyed, Step};

/// Every item once, in list order, on the caller's thread; the answer is the first `Err`. Errors
/// do not cancel later items; a panic unwinds without admitting them, and there is nothing to join.
pub fn parallel<I: Copy + Send, C: Sync, E: Send>(
    cx: &C,
    items: &[I],
    body: impl Fn(I, &C) -> Result<(), E> + Sync,
) -> Result<(), E> {
    let mut first = None;
    for &item in items {
        first = first.or(body(item, cx).err());
    }
    first.map_or(Ok(()), Err)
}

/// Every item once, in list order, each on its key's slot. List order keeps each key's history and
/// makes the first error met the first in list order; an out-of-range key is an error without a
/// call. Key grouping belongs with the SIMD wave lowering that will consume it.
pub fn ordered<I: Copy + Send, K: Copy + Into<usize>, T: Send, C: Sync, E: Send>(
    cx: &C,
    items: &[I],
    keyed: Keyed<'_, K, T>,
    key: impl Fn(I) -> K,
    body: impl Fn(I, &mut T, &C) -> Result<(), E> + Sync,
) -> Result<(), Invoked<E>> {
    let slots = keyed.into_slots();
    let len = slots.len();
    let mut first = None;
    for &item in items {
        let answer = match key(item).into() {
            key if key < len => body(item, &mut slots[key], cx).map_err(Invoked::Failed),
            key => Err(Invoked::OutOfRange { key, len }),
        };
        first = first.or(answer.err());
    }
    first.map_or(Ok(()), Err)
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
        join(arms.running)
    })
}

/// As `Arms`, each arm also holding its own endpoint `I`.
pub struct IoArms<'scope, 'env: 'scope, I, E, const N: usize> {
    scope: &'scope Scope<'scope, 'env>,
    cut: Arc<AtomicBool>,
    running: [Option<ScopedJoinHandle<'scope, Result<(), E>>>; N],
    ios: [Option<I>; N],
    at: usize,
}

impl<'scope, 'env, I: Send + 'scope, E: Send + 'scope, const N: usize> IoArms<'scope, 'env, I, E, N> {
    pub fn arm<F>(&mut self, arm: &'scope mut F)
    where
        F: FnMut(&mut I) -> Result<Step, E> + Send,
    {
        let cut = Arc::clone(&self.cut);
        let mut io = self.ios[self.at].take().expect("one endpoint per arm");
        self.running[self.at] = Some(self.scope.spawn(move || drive(&cut, &mut || arm(&mut io))));
        self.at += 1;
    }
}

/// `concurrent` with arm `i` stepped on `ios[i]`. A host-thread backend lends its endpoints and
/// calls this; which endpoint type it lends is its own business.
pub fn spawn<'env, I: Send + 'env, B, E: Send, const N: usize>(ios: [I; N], body: B) -> Result<(), E>
where
    B: for<'scope> FnOnce(&mut IoArms<'scope, 'env, I, E, N>),
{
    let ios = ios.map(Some);
    thread::scope(|scope| {
        let mut arms = IoArms {
            scope,
            cut: Arc::new(AtomicBool::new(false)),
            running: std::array::from_fn(|_| None),
            ios,
            at: 0,
        };
        let cut = Arc::clone(&arms.cut);
        let _cut = CutOnPanic(&cut);
        body(&mut arms);
        join(arms.running)
    })
}

/// Join every arm; a panic is resumed after all have joined, else the first error in source order.
fn join<E, const N: usize>(running: [Option<ScopedJoinHandle<'_, Result<(), E>>>; N]) -> Result<(), E> {
    let mut panic = None;
    let mut first = Ok(());
    for thread in running.into_iter().flatten() {
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

// The host lowering of `#[parallel]`, scoped threads over the keys that have work, and of
// `concurrent!`, one scoped thread per arm.

use std::collections::BTreeMap;
use std::panic::resume_unwind;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::thread::{self, Scope, ScopedJoinHandle};

use crate::{Invoked, Keyed, Step};

/// The failure with the least item ordinal, and that ordinal.
type Failure<E> = Option<(usize, E)>;

fn least<E>(a: Failure<E>, b: Failure<E>) -> Failure<E> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.0 < a.0 { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// Run each group on its own scoped thread, the first on the caller's, and join them all. The
/// answer is the failure of least ordinal; a panic is resumed once every thread has been joined.
fn dispatch<G: Send, E: Send>(groups: Vec<G>, run: impl Fn(G) -> Failure<E> + Sync) -> Failure<E> {
    thread::scope(|scope| {
        let mut groups = groups.into_iter();
        let first = groups.next()?;
        let spawned: Vec<_> = groups.map(|group| scope.spawn(|| run(group))).collect();
        let mut best = run(first);
        for thread in spawned {
            best = least(best, thread.join().unwrap_or_else(|payload| resume_unwind(payload)));
        }
        best
    })
}

/// Split `jobs` into at most `threads` groups of near equal size.
fn share<J>(mut jobs: Vec<J>, threads: usize) -> Vec<Vec<J>> {
    let each = jobs.len().div_ceil(threads.max(1)).max(1);
    let mut groups = Vec::new();
    while !jobs.is_empty() {
        groups.push(jobs.split_off(jobs.len().saturating_sub(each)));
    }
    groups
}

/// The threads a call may use: what the host offers, and a failure to learn it is not a `1`.
fn threads() -> usize {
    thread::available_parallelism().expect("the host reports its parallelism").get()
}

/// Every item once, on as many threads as the host offers; the answer is the first `Err` in list
/// order. There is no order between items, so the body may only mutate through shared primitives.
pub fn parallel<I: Copy + Send, C: Sync, E: Send>(
    cx: &C,
    items: &[I],
    body: impl Fn(I, &C) -> Result<(), E> + Sync,
) -> Result<(), E> {
    parallel_on(threads(), cx, items, body)
}

pub fn parallel_on<I: Copy + Send, C: Sync, E: Send>(
    threads: usize,
    cx: &C,
    items: &[I],
    body: impl Fn(I, &C) -> Result<(), E> + Sync,
) -> Result<(), E> {
    let numbered: Vec<(usize, I)> = items.iter().copied().enumerate().collect();
    let run = |group: Vec<(usize, I)>| group.into_iter().fold(None, |first, (at, item)| least(first, body(item, cx).err().map(|e| (at, e))));
    match dispatch(share(numbered, threads), run) {
        Some((_, e)) => Err(e),
        None => Ok(()),
    }
}

/// Every item once, each key's items in list order on one thread at a time; the answer is the
/// first `Err` in list order, an out-of-range key counting as one. Distinct keys run in parallel.
pub fn ordered<I: Copy + Send, K: Copy + Into<usize>, T: Send, C: Sync, E: Send>(
    cx: &C,
    items: &[I],
    keyed: Keyed<'_, K, T>,
    key: impl Fn(I) -> K,
    body: impl Fn(I, &mut T, &C) -> Result<(), E> + Sync,
) -> Result<(), Invoked<E>> {
    ordered_on(threads(), cx, items, keyed, key, body)
}

pub fn ordered_on<I: Copy + Send, K: Copy + Into<usize>, T: Send, C: Sync, E: Send>(
    threads: usize,
    cx: &C,
    items: &[I],
    keyed: Keyed<'_, K, T>,
    key: impl Fn(I) -> K,
    body: impl Fn(I, &mut T, &C) -> Result<(), E> + Sync,
) -> Result<(), Invoked<E>> {
    let slots = keyed.into_slots();
    let len = slots.len();
    let mut runs: BTreeMap<usize, Vec<(usize, I)>> = BTreeMap::new();
    let mut astray = None;
    for (at, &item) in items.iter().enumerate() {
        let key = key(item).into();
        match key < len {
            true => runs.entry(key).or_default().push((at, item)),
            false => astray = astray.or(Some((at, key))),
        }
    }
    // Each key with work is paired with its one slot, walking the slots once in key order.
    let mut cursor = slots.iter_mut();
    let mut next = 0;
    let mut jobs = Vec::with_capacity(runs.len());
    for (key, run) in runs {
        jobs.push((cursor.nth(key - next).expect("a key below the length names a slot"), run));
        next = key + 1;
    }
    let run = |group: Vec<(&mut T, Vec<(usize, I)>)>| {
        let mut first = None;
        for (slot, run) in group {
            for (at, item) in run {
                first = least(first, body(item, slot, cx).err().map(|e| (at, e)));
            }
        }
        first
    };
    let failed = dispatch(share(jobs, threads), run).map(|(at, e)| (at, Invoked::Failed(e)));
    let astray = astray.map(|(at, key)| (at, Invoked::OutOfRange { key, len }));
    match least(failed, astray) {
        Some((_, e)) => Err(e),
        None => Ok(()),
    }
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

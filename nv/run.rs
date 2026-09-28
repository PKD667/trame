//! The warp lowering of `#[parallel]`. Every lane of the calling warp enters with its own locals,
//! so `cx` is lane-private, and each lane returns the first `Err` of its own items in list order:
//! the host model has no collective to bring another lane's answer across.
//!
//! `concurrent!` runs every arm on the calling warp, since a participant is one warp: round-robin
//! in source order, one step per live arm per turn.

use super::warp::{self, LANES, Split};
use super::{Context, Io};
use crate::invoke::{Owner, Receive};
use crate::{Invoked, Keyed, Step};

/// Lane `k` runs items `k`, `k + 32`, … of the list.
pub fn parallel<I: Copy, C, E>(
    cx: &mut C,
    items: &[I],
    mut body: impl FnMut(I, &mut C) -> Result<(), E>,
) -> Result<(), E> {
    warp::sync();
    let mut first = Ok(());
    let mut share = Split::new(0, items.len());
    while share.more() {
        let at = share.at();
        share.step();
        let outcome = body(items[at], cx);
        if first.is_ok() {
            first = outcome;
        }
    }
    warp::sync();
    first
}

/// Every lane walks the whole list and runs the items whose key is its own modulo [`LANES`], so
/// one key stays on one lane in issue order.
pub fn ordered<I: Copy, K: Copy + Into<usize>, T, C, E>(
    cx: &mut C,
    items: &[I],
    mut keyed: Keyed<'_, K, T>,
    key: impl Fn(I) -> K,
    mut body: impl FnMut(I, &mut T, &mut C) -> Result<(), E>,
) -> Result<(), Invoked<E>> {
    warp::sync();
    let lane = warp::lane() as usize;
    let mut first = Ok(());
    for &item in items {
        let key = key(item);
        if key.into() % LANES as usize != lane {
            continue;
        }
        let outcome = match keyed.slot(key) {
            Ok(slot) => body(item, slot, cx).map_err(Invoked::Failed),
            Err(out) => Err(out),
        };
        if first.is_ok() {
            first = outcome;
        }
    }
    warp::sync();
    first
}

/// Fixes an arm's closure signature where it is written. `Send` keeps it portable across lowerings.
#[inline(always)]
pub fn arm<F, E>(arm: F) -> F
where
    F: FnMut() -> Result<Step, E> + Send,
{
    arm
}

pub struct Arms<E, const N: usize> {
    done: [bool; N],
    at: usize,
    failed: Option<E>,
}

impl<E, const N: usize> Arms<E, N> {
    pub fn arm<F>(&mut self, arm: &mut F)
    where
        F: FnMut() -> Result<Step, E> + Send,
    {
        let at = self.at;
        self.at += 1;
        if self.failed.is_some() || self.done[at] {
            return;
        }
        let step = arm();
        uniform(&step);
        match step {
            Ok(Step::Progress | Step::Idle) => {}
            Ok(Step::Done) => self.done[at] = true,
            Err(e) => self.failed = Some(e),
        }
    }
}

/// The first error ends the run before any other arm steps, so it is the only one recorded.
pub fn concurrent<B, E: Send, const N: usize>(mut body: B) -> Result<(), E>
where
    B: FnMut(&mut Arms<E, N>),
{
    let mut arms = Arms {
        done: [false; N],
        at: 0,
        failed: None,
    };
    loop {
        arms.at = 0;
        body(&mut arms);
        if let Some(e) = arms.failed.take() {
            return Err(e);
        }
        if arms.done.iter().all(|&done| done) {
            return Ok(());
        }
    }
}

/// As `Arms`, lending each arm the whole context for the length of its step.
pub struct IoArms<'c, 'r, E, const N: usize> {
    cx: &'c mut Context,
    receive: &'r [Receive<'r>; N],
    leader_first: [bool; N],
    done: [bool; N],
    at: usize,
    failed: Option<E>,
}

impl<E, const N: usize> IoArms<'_, '_, E, N> {
    pub fn arm<F>(&mut self, arm: &mut F)
    where
        F: for<'p> FnMut(&mut Io<'p>) -> Result<Step, E> + Send,
    {
        let at = self.at;
        self.at += 1;
        if self.failed.is_some() || self.done[at] {
            return;
        }
        let owner = Owner::new(self.receive, at);
        let step = arm(&mut Io::new(&mut *self.cx, owner, &mut self.leader_first[at]));
        uniform(&step);
        match step {
            Ok(Step::Progress | Step::Idle) => {}
            Ok(Step::Done) => self.done[at] = true,
            Err(e) => self.failed = Some(e),
        }
    }
}

/// `concurrent` given a context: the arms take turns on the warp, each lent `cx` for its step.
#[doc(hidden)]
pub fn concurrent_io<'c, 'r, B, E: Send, const N: usize>(
    cx: &'c mut Context,
    receive: &'r [Receive<'r>; N],
    mut body: B,
) -> Result<(), E>
where
    B: FnMut(&mut IoArms<'c, 'r, E, N>),
{
    let mut arms = IoArms {
        cx,
        receive,
        leader_first: [false; N],
        done: [false; N],
        at: 0,
        failed: None,
    };
    loop {
        arms.at = 0;
        body(&mut arms);
        if let Some(e) = arms.failed.take() {
            return Err(e);
        }
        if arms.done.iter().all(|&done| done) {
            return Ok(());
        }
    }
}

/// A step whose lanes disagree would send the warp down two paths of the runner; trap so the
/// driver reports it instead.
#[inline(always)]
fn uniform<E>(step: &Result<Step, E>) {
    #[cfg(feature = "cuda")]
    {
        let code = match step {
            Ok(Step::Progress) => 0,
            Ok(Step::Idle) => 1,
            Ok(Step::Done) => 2,
            Err(_) => 3,
        };
        if !warp::all(warp::shuffle(code, 0) == code) {
            cuda_device::debug::trap();
        }
    }
    #[cfg(not(feature = "cuda"))]
    let _ = step;
}

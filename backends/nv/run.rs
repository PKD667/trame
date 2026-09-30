//! Inline, list-order invocation and round-robin process steps on the logical caller.
//! No value or outcome is broadcast to other physical lanes.

use super::{Context, Io};
use crate::invoke::{Owner, Receive};
use crate::{Invoked, Keyed, Step};

pub fn parallel<I: Copy + Send, C: Sync, E: Send>(
    cx: &C,
    items: &[I],
    body: impl Fn(I, &C) -> Result<(), E> + Sync,
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

pub fn ordered<I: Copy + Send, K: Copy + Into<usize>, T: Send, C: Sync, E: Send>(
    cx: &C,
    items: &[I],
    keyed: Keyed<'_, K, T>,
    key: impl Fn(I) -> K,
    body: impl Fn(I, &mut T, &C) -> Result<(), E> + Sync,
) -> Result<(), Invoked<E>> {
    let slots = keyed.into_slots();
    let len = slots.len();
    let mut first = Ok(());
    for &item in items {
        let key = key(item).into();
        let outcome = match slots.get_mut(key) {
            Some(slot) => body(item, slot, cx).map_err(Invoked::Failed),
            None => Err(Invoked::OutOfRange { key, len }),
        };
        if first.is_ok() {
            first = outcome;
        }
    }
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
        match step {
            Ok(Step::Progress | Step::Idle) => {}
            Ok(Step::Done) => self.done[at] = true,
            Err(e) => self.failed = Some(e),
        }
    }
}

/// The arms take turns on the caller, each lent `cx` for one bounded step.
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

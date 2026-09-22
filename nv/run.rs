//! The warp lowering of `#[parallel]`. Every lane of the calling warp enters with its own locals,
//! so `cx` is lane-private, and each lane returns the first `Err` of its own items in list order:
//! the host model has no collective to bring another lane's answer across.
//!
//! There is no `concurrent` here. A participant is one warp (`warp::here_id` is the launch index
//! over [`LANES`]), so a launch has no second warp to give one participant for another item, and
//! the attribute refuses `#[concurrent]` under `nv` by name.

use super::warp::{self, LANES, Split};
use crate::{Invoked, Keyed};

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

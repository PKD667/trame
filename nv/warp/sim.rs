//! The host model of a warp: 32 sequential passes of one body over the same memory.
//!
//! What this can model, and what it deliberately cannot:
//!
//! * **The lane identity.** [`lane`] answers `0..32` while [`sim`] drives the passes, so a
//!   `#[parallel]` walk takes its stride from a real lane number and a test sees
//!   the same partition the hardware would.
//! * **The index split.** Everything `#[parallel]` lowers to is arithmetic over [`lane`] and
//!   [`LANES`](super::LANES), so the split is exercised in full here.
//! * **Not the collectives.** `any`, `all`, `ballot` and `shuffle` move a value from one lane's
//!   register to another's *during* a pass. A sequence of whole-warp passes has already finished
//!   lane 0 before lane 1 starts, so there is no instant at which they could meet. This module
//!   therefore does not define them at all: `warp::shuffle` is a compile error without the
//!   `cuda` feature. A stub that returned something would turn "not modelled" into "wrong
//!   answer at run time", which is the failure mode worth refusing.

use core::cell::Cell;

std::thread_local! {
    /// The lane the current pass is playing. Set by [`sim`], read by [`lane`].
    static LANE: Cell<u32> = const { Cell::new(0) };
    /// The warp the current pass belongs to. Set by [`sim_warp`], read by [`here_id`].
    static WARP: Cell<u32> = const { Cell::new(0) };
}

/// This lane's index within its warp, `0..`[`LANES`](super::LANES).
///
/// Returns 0 outside [`sim`], which is the identity of a single host caller: a `#[parallel]`
/// invoked with no warp around it runs the lane-0 share of its list and nothing else.
#[inline(always)]
pub fn lane() -> u32 {
    LANE.with(Cell::get)
}

/// This warp's index. Returns 0 outside [`sim_warp`]: a host caller with no launch around it is
/// the first warp of a one-warp world.
#[inline(always)]
pub fn here_id() -> u32 {
    WARP.with(Cell::get)
}

/// Run `body` as warp `id`, for a test that models more than one warp in one process.
///
/// The warp's identity is a *context*, not a thread: the model runs warps one after another on
/// one thread, so this says which warp the pass that follows belongs to, and nothing about
/// concurrency. A test that needs two warps to run at once needs the device.
#[inline(always)]
pub fn sim_warp<R>(id: u32, body: impl FnOnce() -> R) -> R {
    let previous = WARP.with(|w| w.replace(id));
    let out = body();
    WARP.with(|w| w.set(previous));
    out
}

/// A rendezvous of this warp's lanes.
///
/// A no-op in the model, and soundly so: the model has already finished every lane's pass before returning from it, so every lane has arrived by the time any caller looks.
/// The collectives that move a *value* between lanes are the ones this model cannot stand in
/// for, and they are absent rather than no-ops.
#[inline(always)]
pub fn sync() {}

/// Run `body` once per lane, in lane order, over the same memory, and collect one result per
/// lane.
///
/// The passes are sequential and the memory is shared, so a body that writes an index the split
/// assigned to another lane is a race the model reports as a wrong final value rather than as a
/// hang. That is the honest limit of the model: it checks the partition, not the concurrency.
#[inline(always)]
pub fn sim<R>(mut body: impl FnMut() -> R) -> Vec<R> {
    let mut out = Vec::with_capacity(super::LANES as usize);
    for k in 0..super::LANES {
        LANE.with(|l| l.set(k));
        out.push(body());
    }
    LANE.with(|l| l.set(0));
    out
}

/// Run `body` as one named lane, for a test that wants a single pass. The calling lane is
/// restored afterwards.
#[inline(always)]
pub fn sim_lane<R>(k: u32, body: impl FnOnce() -> R) -> R {
    assert!(k < super::LANES, "a warp has {} lanes", super::LANES);
    let previous = LANE.with(|l| l.replace(k));
    let out = body();
    LANE.with(|l| l.set(previous));
    out
}

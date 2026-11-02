//! Level markers: the only interface between the macros and the driver (`driver/`).
//!
//! `parallel!` and `#[trame::step]` call a level marker with the code they run, and `#[trame::state]`
//! calls a type marker with the types it holds. The calls do nothing at run time. After
//! monomorphization the driver finds every instance of `item::<F>`, `step::<F>` and `worker::<F>`, and
//! checks `F` and everything it reaches at that level (CONTRACT.md, M4 for a step, spec.md for the
//! rest); it finds every instance of `state::<S>` and `pool::<P>`, and checks the type (CONTRACT.md, M2,
//! M3). Renaming or moving these functions breaks the driver.
//!
//! `#[trame::step]` calls `step` from a hidden non-generic function of its step struct, and
//! `#[trame::state]` calls `state` (and `pool`, when the state names one) from one of its own. A
//! checked build collects every non-generic function, so a step that nothing calls is still checked.

/// `f` runs as a `parallel!` item: no effects, counted loops, no recursion, no allocation.
#[inline(always)]
pub fn item<F>(f: &F) {
    let _ = f;
}

/// `f` is a step (`#[trame::step]`, the stop step included): effects of `Σ_C` only, counted loops, no
/// recursion, no allocation (CONTRACT.md, M4). The driver checks `f` and what it reaches.
#[inline(always)]
pub fn step<F>(f: &F) {
    let _ = f;
}

/// `f` runs on the leader's side, as a builder or an edit: effects of `Σ_W`, no OS, allocation
/// allowed. No code in the library emits it yet: `Topology::set` and `Run::edit` would call it.
///
/// `#[inline(never)]`: those would call it from trame's own crate, which is not built with
/// `-Zinline-mir=no`. An optimized build would inline this empty call away, and the driver would find
/// no worker root. The other markers are called from programs, which keep the call.
#[inline(never)]
pub fn worker<F>(f: &F) {
    let _ = f;
}

/// `S` is a `#[trame::state]` struct: no field holds what CONTRACT.md, M2 and M3 exclude. The driver
/// reads `S`'s fields; the call does nothing.
#[inline(always)]
pub fn state<S>() {}

/// `P` is the pool type of a `#[trame::state]` (`pool = P`), checked as a state's field is.
#[inline(always)]
pub fn pool<P>() {}

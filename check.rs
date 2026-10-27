//! Level markers: the only interface between the macros and the driver (`driver/`).
//!
//! `parallel!`, `concurrent!` and `Topology::worker` call one of these with the code they run. The
//! calls do nothing at run time. After monomorphization the driver finds every instance of
//! `item::<F>`, `step::<P>` and `worker::<F>`, and checks `F`, `P::step` and everything they reach
//! at that level of `spec.md` (Compile time). Renaming or moving these functions breaks the driver.

/// `f` runs as a `parallel!` item: no effects, counted loops, no recursion, no allocation.
#[inline(always)]
pub fn item<F>(f: &F) {
    let _ = f;
}

/// `P::step` runs as a `concurrent!` step: effects of `Σ_C` only, counted loops, no recursion. The
/// argument is the process value, so `P` is inferred from it; the driver reads only `P`.
#[inline(always)]
pub fn step<P>(p: &P) {
    let _ = p;
}

/// `f` runs as a worker's `run`: effects of `Σ_W` only, no OS.
///
/// `#[inline(never)]`: `Topology::worker` (topology.rs) calls this from trame's own crate, which is
/// not built with `-Zinline-mir=no`. An optimized build would inline this empty call away, and the driver
/// would find no worker root. The other markers are called from programs, which keep the call.
#[inline(never)]
pub fn worker<F>(f: &F) {
    let _ = f;
}

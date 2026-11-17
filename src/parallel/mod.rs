//! The runtime of `parallel!` (INTERFACE.md 5.4), shared by every backend. It runs the items in one
//! thread. That is a valid schedule: keys are independent, and the items of one key run in input
//! order.
//!
//! It works on the data in place and allocates nothing: one pass checks every key, a second folds
//! each item into its slot in input order, which is each key's input order. Keys interleave, which is
//! allowed: they are independent. A key is an expression over its item, so evaluating it twice
//! answers the same.

// The runtime of workers is worker.rs; its items are re-exported here so every runtime entry is
// `trame::__rt::...`, as the driver's effects.txt names it.
pub use crate::cells::{MAX_OWNED, Owning, Tags, owns};
pub use crate::sched::{POOL, Weight};
pub use crate::worker::{State, StepInfo};

/// Whether the selected backend follows the scheduler's choice exactly (`Backend::EXACT`). Conformance
/// tests read it to decide how a trace is replayed.
pub const EXACT: bool = <crate::backend::Selected as crate::backend::Backend>::EXACT;

/// How many topologies one process can launch (`Backend::RUNS_PER_PROCESS`). A test that loops over seeds
/// takes at most this many.
pub const RUNS_PER_PROCESS: usize = <crate::backend::Selected as crate::backend::Backend>::RUNS_PER_PROCESS;

/// Whether a frame of type `ty` is available to this worker. `#[trame::state]` calls it.
pub fn offered(ty: core::any::TypeId) -> bool {
    use crate::backend::Backend;
    crate::backend::Selected::worker_offer(ty)
}

/// `parallel!` (INTERFACE.md 5.4): for each key `k`, fold the items whose key is `k` into `slots[k]`,
/// in input order. Aborts before any item runs, unless every key is below `slots.len()`, as `x[i]`
/// does (INTERFACE.md 5.4, Parallel effect).
pub fn parallel<I, S, C, K, F>(key: K, f: F, cx: &C, items: &[I], slots: &mut [S])
where
    K: Fn(&I) -> usize,
    F: Fn(&I, &mut S, &C),
{
    let n = slots.len();
    for item in items {
        let k = key(item);
        // Checked for every item before the first runs: a bad key aborts with no item run (INTERFACE.md 5.4).
        assert!(
            k < n,
            "trame: parallel!: an item has key {k}, but there are {n} slots; a key must be below the slot count, as x[i] is. No item ran."
        );
    }
    for item in items {
        f(item, &mut slots[key(item)], cx);
    }
}

// The lane sub-backends (features `lanes-avx2`, `lanes-avx512`; lane.rs). Nothing calls them yet: `parallel!`
// still expands to the scalar fold above. They are declared here, after the runtime, so the runtime's own items
// are the same with or without a lane feature.
#[cfg(feature = "lanes-avx2")]
pub mod avx2;
#[cfg(feature = "lanes-avx512")]
pub mod avx512;
#[cfg(any(feature = "lanes-avx2", feature = "lanes-avx512"))]
pub mod lane;

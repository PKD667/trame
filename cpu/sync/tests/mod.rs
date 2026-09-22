// Tests for the three primitive families, one module per subject.
//
// `bounds` checks what the handles may and may not do across threads, `cost` counts what they
// allocate, `turn`, `publish` and `handoff` check the protocols, and `interleaving` runs the
// small ones under `loom`, which enumerates thread interleavings instead of hoping to hit one.
// The loom module is compiled only under `--cfg loom`; everything else is compiled only when it
// is not, because loom replaces the threads these tests use.

#[cfg(not(loom))]
mod atomic;
#[cfg(not(loom))]
mod bounds;
#[cfg(not(loom))]
mod cost;
#[cfg(not(loom))]
mod handoff;
#[cfg(loom)]
mod interleaving;
#[cfg(not(loom))]
mod publish;
#[cfg(not(loom))]
mod turn;

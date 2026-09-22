// Family C: one atomic word with an ordering named at every call.
//
// There is nothing to implement. The conventional family is what the standard library already
// provides, and a wrapper around it would add a name, a level of indirection and an opportunity
// to change an ordering by accident, while adding no guarantee. So this module re-exports, and
// the application keeps its own named wrappers — `Flag`, `Num`, `Size`, `Tally`, `Tail` in
// `src/nerve/shared.rs` — because those carry the *class* of the value (signal, value, exact
// count, telemetry), which the atomic type cannot.
//
// A later conversion is therefore an import change and nothing else: `src/nerve/shared.rs`
// swaps `use std::sync::atomic::{AtomicU64, Ordering::Acquire, ...}` for the same names out of
// `trame::sync::atomic`, and every ordering stays exactly as it is. Strengthening or weakening
// one is a separate edit with its own reason and its own test; it is not a side effect of moving
// a call site to Family A or B.
//
// Scope. These are CPU atomics. They order accesses by threads that share this process's
// storage, and they say nothing whatsoever about another rank, another host, or a device: no
// amount of `SeqCst` makes a remote rank observe anything. There is no runtime scope enum here
// because on CPU there is one scope. A device backend must declare the scopes it supports before
// it can offer this family; `nv/`'s device-scope acquire/release is that declaration for the
// GPU side and is not part of this module.
//
// What atomics are not: they carry no payload, a CAS retry loop is not a fairness
// argument, `compare_exchange_weak` may fail spuriously, and a spin hint exists because spinning
// has no progress guarantee. Publishing a structure needs `sync::publish`; deciding who mutates
// next needs `sync::turn`.

// Four widths and the ordering enum, and no more. Another width is one name added here by the
// call site that needs it, not a new abstraction — and the set is deliberately the one a *device*
// can also answer, so that `sync::atomic` means the same thing under every backend.
//
// That is why `AtomicBool`, `AtomicUsize` and the standalone `fence` are not here, although the
// host has all three. A device has no boolean or pointer-width atomic of its own — a `usize` is
// 64 bits on that target, so `AtomicU64` is the same word, but naming it `usize` would promise a
// width the target does not have — and this crate offers no device fence. A family whose names
// some backends cannot answer is the thing a declared capability used to hide; the four widths
// are the intersection, and an application that needs a fence for a protocol owns that fence in
// the primitive that implements the protocol rather than reaching for a portable name.
// `AtomicBool` is here and `AtomicUsize` is not, and the difference is that a boolean atomic is a
// thing a worker needs while a pointer-width one is a size NERVE states in `u32`. A boolean is
// also *not* a narrow integer: it carries the operations over `bool`, so a source that says
// boolean is not reading a width. On a device it occupies a 32-bit word underneath, which is a
// fact about that machine and not a promise this name makes.
pub use core::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering};

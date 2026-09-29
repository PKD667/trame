// Backend interface for the portable worker. `backend.md` is the contract and this crate is every
// implementation of it that is selected at build time rather than at run time.
//
// The shape of the crate is the shape of the contract's two halves:
//
//   `contract.rs`  the values every backend shares — `Addr`, `Frame`, `Error`, `Deployment`.
//                  Nothing here chooses a backend.
//   `none/ mpi/ rma/ lossy/ nv/`  one module per backend, each answering the *whole* surface:
//                  entry, identity, communication, geometry, lifetime, clocks, and the primitive
//                  families it supports.
//
// A backend module is the whole answer on purpose. A backend that borrowed half its surface from
// a shared module and supplied the rest would be a second code path for the same thing, and the
// fault would no longer be unambiguously in the wire or outside it. `mpi/`, `rma/` and `lossy/`
// do share their entry and point-to-point halves through `shared/` — they are three lane
// transports over one MPI environment, and that sharing is the environment's, not a shortcut —
// but each re-exports what it borrows so that the selected module answers for everything.
//
// `cpu/` is not part of the surface. It is what a backend whose participants are host threads
// answers: a mutex is a mutex because one process's threads share an address space, a stage
// starts an OS thread, and a monotonic origin is the host's. A backend whose participants are not
// host threads has none of it and supplies its own, which is why `cpu::sync` and
// `cpu::clock` are imports of `none/`, `mpi/`, `rma/` and `lossy/` and never of `nv/`.
//
// One backend is selected at build time. The types are values; there is no trait object, no
// run-time dispatch and no registry.

mod contract;
pub use contract::{
    Addr, Backend, BackendFault, Channel, ClockId, ClockMismatch, Deployment, Edge, Error, Failure,
    FailureKind, Frame, Handle, Invalid, Launch, Participant, Reading, Span, Tag,
};

// Generated code says `::trame::...` whether it was expanded in an application or in this
// crate's own tests, so this crate has to answer to its own library name too.
extern crate self as trame;

// The device backend answers none of it, so under `nv` it is only its own tests' subject.
#[cfg(not(feature = "nv"))]
mod cpu;

// How a unit of work is run. The attributes live in `macros/`; which lowering a driver or
// `concurrent!` calls is the same compile-time choice as the transport.
mod invoke;
pub use invoke::{Invocation, Invoked, Keyed, Step};
#[doc(hidden)]
pub use invoke::{Receive, arm_io};
pub use trame_macros::{ordered, parallel, process};

// The MPI environment and point-to-point traffic, shared by the three lane transports that ride
// on it. Not compiled without MPI: it names no model type, but it does name the wire.
#[cfg(feature = "mpi")]
mod shared;

#[cfg(not(any(feature = "mpi", feature = "nv")))]
mod none;

// The host model and the device build each use part of this, and the device measurements
// (`nv/measure/`) use the rings below the contract, so what one build leaves unused is not dead.
#[cfg(feature = "nv")]
#[allow(dead_code, unused_imports)]
mod nv;

#[cfg(feature = "rma-lossy")]
mod lossy;
#[cfg(all(feature = "mpi", not(feature = "ring")))]
mod mpi;
#[cfg(all(feature = "ring", not(feature = "rma-lossy")))]
mod rma;

#[cfg(test)]
mod tests;

// Select one complete implementation, then export the contract surface once. A missing name in
// any selected module is therefore a compile error at this boundary rather than at a call site.
#[cfg(feature = "rma-lossy")]
use lossy as selected;
#[cfg(all(feature = "mpi", not(feature = "ring")))]
use mpi as selected;
#[cfg(not(any(feature = "mpi", feature = "nv")))]
use none as selected;
#[cfg(feature = "nv")]
use nv as selected;
#[cfg(all(feature = "ring", not(feature = "rma-lossy")))]
use rma as selected;

// There is no `exec` here. Starting a named body on another host thread is the host lowering's
// own business. `sync` remains in the surface because Family A is a backend family.
pub use selected::{
    Context, Environment, Io, Shared, attach, barrier, bytes, clock, detach, done, flush, init,
    leader, rank, recv, release, reshape, send, size, sync,
};
#[doc(hidden)]
pub use selected::{concurrent_io, run};

/// Which backend this build selected.
pub const ID: Backend = selected::ID;

/// Whether this backend's lanes may lose a frame. The `Message` route is reliable regardless, so
/// this is a statement about lanes and never about control traffic.
pub const LOSSY: bool = matches!(ID, Backend::RmaLossy);

/// The longest frame every route of this backend carries, leader route included. The selected
/// backend states which storage provides it and refuses a launch that does not.
pub const MAX_FRAME: usize = selected::MAX_FRAME;

// A 64 KiB log block behind an eight-byte batch header is 65_544 bytes, and every route must
// carry one. The upper bound is what lets a backend narrow `MAX_FRAME` to the `u32` its wire or
// slot header carries without a fallible conversion.
const _: () = assert!(MAX_FRAME >= 65_544 && MAX_FRAME <= u32::MAX as usize);

// Compile-time checks that the selection is coherent. The three lane transports ride one MPI
// environment, and the device backend rides none of it, so the combinations that would ask one
// executable to be two wires at once are refused here rather than at a link error.
const _: () = assert!(!cfg!(feature = "nv") || !cfg!(feature = "mpi"));
const _: () = assert!(!cfg!(feature = "ring") || cfg!(feature = "mpi"));

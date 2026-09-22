// Backend interface for the portable worker. `backend.md` is the contract and this crate is every
// implementation of it that is selected at build time rather than at run time.
//
// The shape of the crate is the shape of the contract's two halves:
//
//   `contract.rs`  the values every backend shares — `Rank`, `Frame`, `Error`, the capabilities,
//                  and `require!`. Nothing here chooses a backend.
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
    Channel, ClockId, Declarations, Deployment, Edge, Error, Failure, Frame, LANE_LOSSY,
    LANE_RELIABLE, LANE_UNAVAILABLE, PUBLICATION_COLLECTIVE, PUBLICATION_PREPUBLISHED,
    PUBLICATION_UNAVAILABLE, RELEASE_COHORT, RELEASE_LOCAL, RELEASE_UNAVAILABLE, Rank, Reading,
    Scopes, Span, Tag, Wait,
};

// The byte-range rule every published segment is cut by, shared because the origin and every
// participant must reach the same answer without communicating. Pure, and the only definition of
// where the cuts are.
pub mod partition;

// Generated code says `::trame::...` whether it was expanded in an application or in this
// crate's own tests, so this crate has to answer to its own library name too.
extern crate self as trame;

pub mod cpu;

// How a unit of work is run. The attributes live in `macros/`; which lowering a driver calls is the
// same compile-time choice as the transport.
mod invoke;
pub use invoke::{Invocation, Invoked, Keyed};
pub use trame_macros::{concurrent, ordered, parallel};

// The MPI environment and point-to-point traffic, shared by the three lane transports that ride
// on it. Not compiled without MPI: it names no model type, but it does name the wire.
#[cfg(feature = "mpi")]
mod shared;

#[cfg(not(any(feature = "mpi", feature = "nv")))]
pub mod none;

#[cfg(feature = "nv")]
pub mod nv;

#[cfg(feature = "rma-lossy")]
pub mod lossy;
#[cfg(all(feature = "mpi", not(feature = "ring")))]
pub mod mpi;
#[cfg(all(feature = "ring", not(feature = "rma-lossy")))]
pub mod rma;

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
    Context, DECLARATIONS, Environment, ID, Shared, bytes, clock, cohort, done, flush, hosts, init,
    leader, rank, reading, recv, release, reshape, send, share, size, slice, sync, unshare,
};
#[doc(hidden)]
pub use selected::run;

/// Whether this backend's lanes may lose a frame. The communication rules keep the `Message` route
/// reliable regardless, so this is a statement about lanes and never about control traffic.
pub const LOSSY: bool = DECLARATIONS.lane_reliability == LANE_LOSSY;

/// The largest tag the transport distinguishes, from the selected backend's declaration.
pub const TAG_LIMIT: Tag = DECLARATIONS.tag_limit;

// Compile-time checks that the selection is coherent. The three lane transports ride one MPI
// environment, and the device backend rides none of it, so the combinations that would ask one
// executable to be two wires at once are refused here rather than at a link error.
const _: () = assert!(!cfg!(feature = "nv") || !cfg!(feature = "mpi"));
const _: () = assert!(!cfg!(feature = "ring") || cfg!(feature = "mpi"));
const _: () = assert!(!LOSSY || cfg!(feature = "ring"));

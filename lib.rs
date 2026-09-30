// Backend-neutral contract with one compile-time-selected implementation.
mod contract;
pub use contract::{
    Addr, Backend, BackendFault, Channel, ClockId, ClockMismatch, Deployment, Edge, Error, Failure,
    FailureKind, Frame, Handle, Invalid, Launch, Participant, Reading, Span, Tag,
};
extern crate self as trame;

#[path = "optim/mod.rs"]
mod portable_optim;

#[cfg(not(feature = "nv"))]
#[path = "backends/host/mod.rs"]
mod host;
mod invoke;
pub use invoke::{Invocation, Invoked, Keyed, Step};
#[doc(hidden)]
pub use invoke::{Receive, arm_io};
pub use trame_macros::{ordered, parallel, process};

#[cfg(feature = "mpi")]
#[path = "backends/mpi/mod.rs"]
mod mpi;
#[cfg(not(any(feature = "mpi", feature = "nv")))]
#[path = "backends/none/mod.rs"]
mod none;
#[cfg(feature = "nv")]
#[allow(dead_code, unused_imports)]
#[path = "backends/nv/mod.rs"]
mod nv;

#[cfg(test)]
mod tests;

#[cfg(feature = "mpi")]
use mpi as selected;
#[cfg(not(any(feature = "mpi", feature = "nv")))]
use none as selected;
#[cfg(feature = "nv")]
use nv as selected;

pub use selected::optim;
pub use selected::{
    Context, Environment, Io, Shared, attach, barrier, bytes, clock, detach, done, flush, init,
    leader, rank, recv, release, reshape, send, size, sync,
};
#[doc(hidden)]
pub use selected::{concurrent_io, run};

/// Which backend this build selected.
pub const ID: Backend = selected::ID;

/// Whether this backend's lanes may lose a frame. The `Message` route remains reliable.
pub const LOSSY: bool = cfg!(feature = "lossy");

/// The longest frame every route of this backend carries, leader route included.
pub const MAX_FRAME: usize = selected::MAX_FRAME;

const _: () = assert!(MAX_FRAME >= 65_544 && MAX_FRAME <= u32::MAX as usize);
const _: () = assert!(!cfg!(feature = "nv") || !cfg!(feature = "mpi"));
const _: () = assert!(!cfg!(feature = "lossy") || cfg!(feature = "mpi"));

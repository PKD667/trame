//! The backends of the tree and the choice among them (CONTRACT.md, Backends in the tree). A build
//! selects one by Cargo feature; the rest of the crate reaches it only through `selected`, as the
//! `Backend` trait of backend.rs. Nothing outside this directory names a backend.

#[cfg(all(feature = "seq", feature = "mpi"))]
compile_error!(
    "features `seq` and `mpi` are exclusive: one backend per build (mpi: --no-default-features --features mpi)"
);

#[cfg(feature = "seq")]
pub(crate) mod seq;

/// The routes of a backend whose workers share one address space.
#[cfg(feature = "seq")]
#[path = "local/routes.rs"]
mod local_routes;

/// One rank per worker, one host per shared-memory node with its leader on the node's last rank
/// (mpi/README.md).
#[cfg(feature = "mpi")]
pub(crate) mod mpi;

#[cfg(all(feature = "seq", not(feature = "mpi")))]
pub(crate) use self::seq as selected;

// `self::`: the crate may also depend on an external crate with the backend's name, and a bare name is ambiguous.
#[cfg(all(feature = "mpi", not(feature = "seq")))]
pub(crate) use self::mpi as selected;

// A build with no backend has nothing to launch on. There is no default: name one.
#[cfg(not(any(feature = "seq", feature = "mpi")))]
compile_error!(
    "select a backend: --no-default-features --features <name>, where <name> is a directory of src/backends/ with a backend.nix"
);

/// The selected backend's own items, for the test binaries under mpi/tests. Not API.
#[doc(hidden)]
pub mod __backend {
    pub use super::selected::*;
}

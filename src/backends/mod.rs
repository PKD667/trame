//! The backends of the tree and the choice among them (CONTRACT.md, Backends in the tree). A build
//! selects one by Cargo feature; the rest of the crate reaches it only through `selected`, as the
//! `Backend` trait of backend.rs. Nothing outside this directory names a backend, and neither does this file:
//! build.rs reads the directories of src/backends/ and generates the declaration of the selected one, or the
//! compile error when none or several are selected (src/backends/README.md, The choice).

include!(concat!(env!("OUT_DIR"), "/backends.rs"));

/// The selected backend's own items, for the test binaries under a backend's tests/. Not API.
#[doc(hidden)]
pub mod __backend {
    pub use super::NAME;
    pub use super::selected::*;
}

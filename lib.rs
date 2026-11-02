//! Trame: one meaning for a program on every machine it runs on. `spec.md` is the contract for
//! humans; this crate is the same contract in Rust. A change to one is a change to the other.
#![no_std]

extern crate alloc;

// The macros expand to `::trame::...`, so the crate's own tests need the same name.
extern crate self as trame;

/// Re-exported so a message's derives name `::trame::__bytemuck`: users need no bytemuck of their own.
#[doc(hidden)]
pub use bytemuck as __bytemuck;

/// Makes a struct a message type: `#[trame::message]`. A message is plain data (`Message`), so the
/// struct must have no padding and no bit pattern that is invalid. A struct with padding, a reference,
/// a raw pointer or a function pointer is refused at compile time; the refusals are cases of
/// rejections/ (the macro reads the field types, since bytemuck would accept a raw pointer). A plain
/// struct is accepted:
///
/// ```
/// #[trame::message]
/// struct Plain {
///     a: u32,
///     b: u32,
/// }
/// ```
pub use trame_macros::message;

/// Marks `fn f(x: &I, slot: &mut S, cx: &C)` as a `parallel!` item, with its key:
/// `#[trame::item(key = x.index as usize)]` (spec.md 5.4).
pub use trame_macros::item;

/// `parallel!(f, &cx, &items, &mut slots)`: apply item `f` to `items` (spec.md 5.4). Without
/// `--cfg trame_checked` it is a compile error; rejections/ tests that. It has no doc test here,
/// because doc tests are built without the flag.
pub use trame_macros::parallel;

/// `#[trame::state(steps(a, b), stop = f, pool = P, error = E)]` on a struct: the struct is a
/// worker's state, and the listed functions are its steps (spec.md 5.3). `stop`, `pool` and `error`
/// are optional. The leader builds the state and declares the worker with `Topology::worker`.
pub use trame_macros::state;

/// `#[trame::step(S, receives = Job, weight = 0.5)]` on a function: a step of the state `S`. Each
/// parameter names a field of `S` and takes it as `&T` (read) or `&mut T` (write), or is `pool: &P`,
/// the host's pool. `receives` and `weight` are optional. The step returns `Status` or
/// `Result<Status, E>`.
pub use trame_macros::step;

/// The runtime the macros call (spec.md 5.3, 5.4). Not for direct use.
#[doc(hidden)]
#[path = "parallel.rs"]
pub mod __rt;

/// Level markers the macros emit and the driver reads. Not for direct use.
#[doc(hidden)]
#[path = "check.rs"]
pub mod __check;

/// Numeric functions, grouped apart from the effects (spec.md 7).
pub mod math;

// The modules are private: the surface is the root re-exports below (spec.md 7). Their definition
// paths (`trame::io::send`, `trame::time::now`, ...) are what the driver's effects.txt names.
mod io;
mod domain;
mod launch;
mod run;
mod time;
mod topology;

/// The surface of spec.md 7: every public name of the crate, at the root and nowhere else.
pub use io::{Full, Message, me, recv, send};
pub use domain::Alloc;
pub use launch::Refused;
pub use run::{Outcome, Run};
pub use status::Status;
pub use time::{Reading, now};
pub use topology::{Handle, LEADER, Pool, Topology, Worker, WorkerId};

#[cfg(any(all(feature = "seq", feature = "mpi"), all(feature = "seq", feature = "threads"), all(feature = "threads", feature = "mpi")))]
compile_error!("features `seq`, `threads` and `mpi` are exclusive: one backend per build (threads: --no-default-features --features threads)");

/// The threads backend: several agents per worker, steps truly in parallel (backends/threads).
#[cfg(feature = "threads")]
#[path = "backends/threads/mod.rs"]
mod threads;

#[cfg(feature = "seq")]
#[path = "backends/seq/mod.rs"]
mod seq;

/// The routes of the backends whose agents share memory (seq, threads).
#[cfg(any(feature = "seq", feature = "threads"))]
#[path = "backends/local/routes.rs"]
mod local_routes;

/// The MPI backend: one rank per worker, the leader on the last rank (backends/mpi/README.md).
/// Stop, edit and pools are not yet carried to other ranks: see docs/worker-model.md, stage 2.
#[cfg(feature = "mpi")]
#[path = "backends/mpi/mod.rs"]
mod mpi;

/// The MPI backend's transport, for the test binaries under backends/mpi/tests. Not API.
#[cfg(feature = "mpi")]
#[doc(hidden)]
pub mod __mpi {
    pub use crate::mpi::*;
}

/// Internal: the seam between the surface and the selected backend (see backend.rs).
mod backend;

/// The scheduling model every backend keeps: spec.md 5.3 (sched.rs).
mod sched;

/// The message types a step receives (spec.md 5.2). Re-exported by __rt.
mod cells;

/// A worker: its state, its steps, and the reference scheduler (spec.md 5.3). Re-exported by __rt.
mod worker;

/// What a step returns, and the condition it waits for (spec.md 5.3).
mod status;

#[cfg(all(feature = "seq", not(feature = "mpi"), not(feature = "threads")))]
use seq as selected;

#[cfg(all(feature = "threads", not(feature = "seq"), not(feature = "mpi")))]
use threads as selected;

// `self::`: the crate also depends on an external crate named `mpi`, and a bare `mpi` is ambiguous.
#[cfg(all(feature = "mpi", not(feature = "seq"), not(feature = "threads")))]
use self::mpi as selected;

/// Without a backend module, every launch is refused (see unselected.rs).
#[cfg(not(any(feature = "seq", feature = "mpi", feature = "threads")))]
mod unselected;
#[cfg(not(any(feature = "seq", feature = "mpi", feature = "threads")))]
use unselected as selected;

// The lib's own test binary installs the wrapper like any trame program (domain.rs).
#[cfg(test)]
extern crate std;
#[cfg(test)]
#[global_allocator]
static TEST_ALLOC: Alloc = Alloc::new(std::alloc::System);
